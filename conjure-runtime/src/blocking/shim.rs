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
use crate::blocking::Body;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use conjure_error::Error;
use conjure_http::client::AsyncWriteBody;
use futures::channel::{mpsc, oneshot};
use futures::{executor, SinkExt, StreamExt};
use hyper::header::HeaderValue;
use std::io::{self, Write};
use std::pin::Pin;

enum BodyKind {
    Fixed(Bytes),
    Streaming {
        content_length: Option<u64>,
        sender: mpsc::Sender<ShimRequest>,
    },
}

pub(crate) struct BodyShim {
    content_type: HeaderValue,
    kind: BodyKind,
}

impl BodyShim {
    pub fn new<T>(body: T) -> (BodyShim, BodyStreamer<T>)
    where
        T: Body,
    {
        let content_type = body.content_type();

        let (kind, streamer) = match body.full_body() {
            Some(body) => (BodyKind::Fixed(body), BodyStreamer::Nop),
            None => {
                let (sender, receiver) = mpsc::channel(1);
                (
                    BodyKind::Streaming {
                        content_length: body.content_length(),
                        sender,
                    },
                    BodyStreamer::Streaming { body, receiver },
                )
            }
        };

        (BodyShim { content_type, kind }, streamer)
    }
}

#[async_trait]
impl AsyncWriteBody<crate::BodyWriter> for BodyShim {
    async fn write_body(
        mut self: Pin<&mut Self>,
        mut w: Pin<&mut crate::BodyWriter>,
    ) -> Result<(), Error> {
        let request_sender = match &mut self.kind {
            BodyKind::Fixed(_) => unreachable!(),
            BodyKind::Streaming { sender, .. } => sender,
        };

        let (sender, mut receiver) = mpsc::channel(1);
        request_sender
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
        let request_sender = match &mut self.kind {
            BodyKind::Fixed(_) => return true,
            BodyKind::Streaming { sender, .. } => sender,
        };

        let (sender, receiver) = oneshot::channel();
        if request_sender
            .send(ShimRequest::Reset(sender))
            .await
            .is_err()
        {
            return false;
        }

        receiver.await.unwrap_or(false)
    }
}

pub(crate) enum ShimRequest {
    Write(mpsc::Sender<BodyPart>),
    Reset(oneshot::Sender<bool>),
}

pub(crate) enum BodyPart {
    Data(Bytes),
    Error(Error),
    Done,
}

pub(crate) enum BodyStreamer<T> {
    Nop,
    Streaming {
        body: T,
        receiver: mpsc::Receiver<ShimRequest>,
    },
}

impl<T> BodyStreamer<T>
where
    T: Body,
{
    pub fn stream(self) {
        let (mut body, mut receiver) = match self {
            BodyStreamer::Nop => return,
            BodyStreamer::Streaming { body, receiver } => (body, receiver),
        };

        while let Some(request) = executor::block_on(receiver.next()) {
            match request {
                ShimRequest::Write(sender) => {
                    let mut writer = BodyWriter::new(sender);
                    let _ = match body.write(&mut writer) {
                        Ok(()) => writer.finish(),
                        Err(e) => writer.send(BodyPart::Error(e)),
                    };
                }
                ShimRequest::Reset(sender) => {
                    let reset = body.reset();
                    let _ = sender.send(reset);
                }
            }
        }
    }
}

/// The blocking writer passed to `Body::write`.
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
    /// Compared to the `Write` implementation, this method avoids some copies if the caller already has the body in
    /// `Bytes` objects.
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
