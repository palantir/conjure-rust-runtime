// Copyright 2020 Palantir Technologies, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use crate::Body;
use bytes::{Bytes, BytesMut};
use conjure_error::Error;
use futures::channel::{mpsc, oneshot};
use futures::{ready, SinkExt, Stream};
use hyper::HeaderMap;
use std::io::Cursor;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{error, fmt, io, mem};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use witchcraft_log::debug;

#[derive(Debug)]
pub(crate) struct BodyError;

impl fmt::Display for BodyError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("error writing body")
    }
}

impl error::Error for BodyError {}

pub(crate) enum BodyPart {
    Chunk(Bytes),
    Done,
}

pub(crate) enum HyperBody {
    Empty,
    Single(Bytes),
    Stream {
        receiver: mpsc::Receiver<BodyPart>,
        polled: Option<oneshot::Sender<()>>,
    },
}

impl HyperBody {
    pub(crate) fn new<T>(body: Option<Pin<&mut T>>) -> (HyperBody, Writer<'_, T>)
    where
        T: ?Sized + Body,
    {
        let body = match body {
            Some(body) => body,
            None => return (HyperBody::Empty, Writer::Nop),
        };

        match body.full_body() {
            Some(body) => (HyperBody::Single(body), Writer::Nop),
            None => {
                let (body_sender, body_receiver) = mpsc::channel(1);
                let (polled_sender, polled_receiver) = oneshot::channel();
                (
                    HyperBody::Stream {
                        receiver: body_receiver,
                        polled: Some(polled_sender),
                    },
                    Writer::Streaming {
                        polled: polled_receiver,
                        body,
                        sender: body_sender,
                    },
                )
            }
        }
    }
}

impl http_body::Body for HyperBody {
    type Data = Cursor<Bytes>;
    type Error = BodyError;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match mem::replace(&mut *self, HyperBody::Empty) {
            HyperBody::Empty => Poll::Ready(None),
            HyperBody::Single(chunk) => Poll::Ready(Some(Ok(Cursor::new(chunk)))),
            HyperBody::Stream {
                mut receiver,
                mut polled,
            } => {
                if let Some(polled) = polled.take() {
                    let _ = polled.send(());
                }

                match Pin::new(&mut receiver).poll_next(cx) {
                    Poll::Ready(Some(BodyPart::Chunk(bytes))) => {
                        *self = HyperBody::Stream { receiver, polled };
                        Poll::Ready(Some(Ok(Cursor::new(bytes))))
                    }
                    Poll::Ready(Some(BodyPart::Done)) => Poll::Ready(None),
                    Poll::Ready(None) => Poll::Ready(Some(Err(BodyError))),
                    Poll::Pending => {
                        *self = HyperBody::Stream { receiver, polled };
                        Poll::Pending
                    }
                }
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, HyperBody::Empty)
    }
}

pub(crate) enum Writer<'a, T>
where
    T: ?Sized,
{
    Nop,
    Streaming {
        polled: oneshot::Receiver<()>,
        body: Pin<&'a mut T>,
        sender: mpsc::Sender<BodyPart>,
    },
}

impl<'a, T> Writer<'a, T>
where
    T: ?Sized + Body,
{
    pub async fn write(self) -> Result<(), Error> {
        match self {
            Writer::Nop => Ok(()),
            Writer::Streaming {
                polled,
                body,
                sender,
            } => {
                // wait for hyper to actually ask for the body so we don't start reading it if the request fails early
                if polled.await.is_err() {
                    debug!("hyper hung up before polling request body");
                    return Ok(());
                }

                let mut writer = BodyWriter::new(sender);
                body.write(Pin::new(&mut writer)).await?;
                writer.finish().await.map_err(Error::internal_safe)?;

                Ok(())
            }
        }
    }
}

/// The asynchronous writer passed to `Body::write_body`.
pub struct BodyWriter {
    sender: mpsc::Sender<BodyPart>,
    buf: BytesMut,
}

impl BodyWriter {
    fn new(sender: mpsc::Sender<BodyPart>) -> BodyWriter {
        BodyWriter {
            sender,
            buf: BytesMut::new(),
        }
    }

    async fn finish(mut self) -> io::Result<()> {
        self.flush().await?;
        self.sender
            .send(BodyPart::Done)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Writes a block of body bytes.
    ///
    /// Compared to the `AsyncWrite` implementation, this method avoids some copies if the caller already has the body
    /// in `Bytes` objects.
    pub async fn write_bytes(&mut self, bytes: Bytes) -> io::Result<()> {
        self.flush().await?;
        self.sender
            .send(BodyPart::Chunk(bytes))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }
}

impl AsyncWrite for BodyWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.buf.len() > 4096 {
            ready!(self.as_mut().poll_flush(cx))?;
        }

        self.buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.buf.is_empty() {
            return Poll::Ready(Ok(()));
        }

        ready!(self.sender.poll_ready(cx)).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let chunk = self.buf.split().freeze();
        self.sender
            .start_send(BodyPart::Chunk(chunk))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
