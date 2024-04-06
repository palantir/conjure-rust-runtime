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
use crate::raw::DefaultRawBody;
use bytes::{Bytes, BytesMut};
use conjure_error::Error;
use conjure_http::client::{AsyncWriteBody, WriteBody};
use futures::channel::{mpsc, oneshot};
use futures::{executor, SinkExt, Stream, StreamExt};
use http_body::Body;
use std::error;
use std::io::{self, BufRead, Read, Write};
use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::runtime::Handle;

pub(crate) fn shim(
    body_writer: Box<dyn WriteBody<BodyWriter> + '_>,
) -> (BodyWriterShim, BodyStreamer<'_>) {
    let (sender, receiver) = mpsc::channel(1);
    (
        BodyWriterShim { sender },
        BodyStreamer {
            body_writer,
            receiver,
        },
    )
}

enum ShimRequest {
    Write(mpsc::Sender<BodyPart>),
    Reset(oneshot::Sender<bool>),
}

pub(crate) struct BodyWriterShim {
    sender: mpsc::Sender<ShimRequest>,
}

impl AsyncWriteBody<crate::BodyWriter> for BodyWriterShim {
    async fn write_body(
        mut self: Pin<&mut Self>,
        mut w: Pin<&mut crate::BodyWriter>,
    ) -> Result<(), Error> {
        let (sender, mut receiver) = mpsc::channel(1);
        self.sender
            .send(ShimRequest::Write(sender))
            .await
            .map_err(Error::internal_safe)?;

        loop {
            match receiver.next().await {
                Some(BodyPart::Data(bytes)) => w
                    .as_mut()
                    .write_bytes(bytes)
                    .await
                    .map_err(Error::internal_safe)?,
                Some(BodyPart::Error(error)) => return Err(error),
                Some(BodyPart::Done) => return Ok(()),
                None => return Err(Error::internal_safe("body write aborted")),
            }
        }
    }

    async fn reset(mut self: Pin<&mut Self>) -> bool {
        let (sender, receiver) = oneshot::channel();
        if self.sender.send(ShimRequest::Reset(sender)).await.is_err() {
            return false;
        }

        receiver.await.unwrap_or(false)
    }
}

pub(crate) struct BodyStreamer<'a> {
    body_writer: Box<dyn WriteBody<BodyWriter> + 'a>,
    receiver: mpsc::Receiver<ShimRequest>,
}

impl BodyStreamer<'_> {
    pub fn stream(mut self) {
        while let Some(request) = executor::block_on(self.receiver.next()) {
            match request {
                ShimRequest::Write(sender) => {
                    let mut writer = BodyWriter::new(sender);
                    let _ = match self.body_writer.write_body(&mut writer) {
                        Ok(()) => writer.finish(),
                        Err(e) => writer.send(BodyPart::Error(e)),
                    };
                }
                ShimRequest::Reset(sender) => {
                    let reset = self.body_writer.reset();
                    let _ = sender.send(reset);
                }
            }
        }
    }
}

enum BodyPart {
    Data(Bytes),
    Error(Error),
    Done,
}

/// The blocking writer passed to [`WriteBody::write_body`].
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

    fn finish(mut self) -> io::Result<()> {
        self.flush()?;
        self.send(BodyPart::Done)
    }

    fn send(&mut self, message: BodyPart) -> io::Result<()> {
        executor::block_on(self.sender.send(message))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    /// Writes a block of body bytes.
    ///
    /// Compared to the [`Write`] implementation, this method avoids some copies if the caller already has the body in
    /// [`Bytes`] objects.
    pub fn write_bytes(&mut self, buf: Bytes) -> io::Result<()> {
        self.flush()?;
        self.send(BodyPart::Data(buf))
    }
}

impl Write for BodyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        if buf.len() > 4906 {
            self.flush()?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }

        let bytes = self.buf.split().freeze();
        self.send(BodyPart::Data(bytes))
    }
}

/// A blocking streaming response body.
pub struct ResponseBody<B = DefaultRawBody> {
    inner: Pin<Box<crate::ResponseBody<B>>>,
    handle: Handle,
}

impl<B> ResponseBody<B> {
    pub(crate) fn new(inner: crate::ResponseBody<B>, handle: Handle) -> Self {
        ResponseBody {
            inner: Box::pin(inner),
            handle,
        }
    }
}

impl<B> Iterator for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Item = Result<Bytes, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.handle.block_on(self.inner.as_mut().next())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<B> Read for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.handle.block_on(self.inner.as_mut().read(buf))
    }
}

impl<B> BufRead for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        // lifetime shenanigans mean we can't return the value of fill_buf directly
        self.handle.block_on(self.inner.as_mut().fill_buf())?;
        Ok(self.inner.buffer())
    }

    fn consume(&mut self, amt: usize) {
        self.inner.as_mut().consume(amt)
    }
}
