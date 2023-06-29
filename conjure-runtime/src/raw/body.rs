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
use crate::BodyWriter;
use bytes::Bytes;
use conjure_error::Error;
use conjure_http::client::{AsyncRequestBody, AsyncWriteBody};
use futures::channel::{mpsc, oneshot};
use futures::{pin_mut, Stream};
use hyper::HeaderMap;
use pin_project::pin_project;
use std::io::Cursor;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{error, fmt, mem};
use witchcraft_log::debug;

/// The error type returned by `RawBody`.
#[derive(Debug)]
pub struct BodyError(());

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

pub(crate) enum RawBodyInner {
    Empty,
    Single(Bytes),
    Stream {
        receiver: mpsc::Receiver<BodyPart>,
        polled: Option<oneshot::Sender<()>>,
    },
}

/// The request body type passed to the raw HTTP client.
#[pin_project]
pub struct RawBody {
    pub(crate) inner: RawBodyInner,
    #[pin]
    _p: PhantomPinned,
}

impl RawBody {
    pub(crate) fn new(body: AsyncRequestBody<'_, BodyWriter>) -> (RawBody, Writer<'_>) {
        match body {
            AsyncRequestBody::Empty => (
                RawBody {
                    inner: RawBodyInner::Empty,
                    _p: PhantomPinned,
                },
                Writer::Nop,
            ),
            AsyncRequestBody::Fixed(body) => (
                RawBody {
                    inner: RawBodyInner::Single(body),
                    _p: PhantomPinned,
                },
                Writer::Nop,
            ),
            AsyncRequestBody::Streaming(body) => {
                let (body_sender, body_receiver) = mpsc::channel(1);
                let (polled_sender, polled_receiver) = oneshot::channel();
                (
                    RawBody {
                        inner: RawBodyInner::Stream {
                            receiver: body_receiver,
                            polled: Some(polled_sender),
                        },
                        _p: PhantomPinned,
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

impl http_body::Body for RawBody {
    type Data = Cursor<Bytes>;
    type Error = BodyError;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        match mem::replace(this.inner, RawBodyInner::Empty) {
            RawBodyInner::Empty => Poll::Ready(None),
            RawBodyInner::Single(chunk) => Poll::Ready(Some(Ok(Cursor::new(chunk)))),
            RawBodyInner::Stream {
                mut receiver,
                mut polled,
            } => {
                if let Some(polled) = polled.take() {
                    let _ = polled.send(());
                }

                match Pin::new(&mut receiver).poll_next(cx) {
                    Poll::Ready(Some(BodyPart::Chunk(bytes))) => {
                        *this.inner = RawBodyInner::Stream { receiver, polled };
                        Poll::Ready(Some(Ok(Cursor::new(bytes))))
                    }
                    Poll::Ready(Some(BodyPart::Done)) => Poll::Ready(None),
                    Poll::Ready(None) => Poll::Ready(Some(Err(BodyError(())))),
                    Poll::Pending => {
                        *this.inner = RawBodyInner::Stream { receiver, polled };
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
        matches!(self.inner, RawBodyInner::Empty)
    }
}

pub(crate) enum Writer<'a> {
    Nop,
    Streaming {
        polled: oneshot::Receiver<()>,
        body: Pin<&'a mut (dyn AsyncWriteBody<BodyWriter> + Send)>,
        sender: mpsc::Sender<BodyPart>,
    },
}

impl<'a> Writer<'a> {
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

                let writer = BodyWriter::new(sender);
                pin_mut!(writer);
                body.write_body(writer.as_mut()).await?;
                writer.finish().await.map_err(Error::internal_safe)?;

                Ok(())
            }
        }
    }
}
