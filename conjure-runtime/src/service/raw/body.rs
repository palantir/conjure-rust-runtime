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
use conjure_http::client::{AsyncRequestBody, AsyncWriteBody, BoxAsyncWriteBody};
use futures::channel::{mpsc, oneshot};
use futures::{pin_mut, Stream};
use http_body::{Frame, SizeHint};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{error, fmt, mem};
use witchcraft_log::debug;

#[derive(Debug)]
pub struct RequestBodyError(());

impl fmt::Display for RequestBodyError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("error writing body")
    }
}

impl error::Error for RequestBodyError {}

pub(crate) enum RequestBodyPart {
    Frame(Frame<Bytes>),
    Done,
}

pub(crate) enum RawRequestBodyInner {
    Empty,
    Single(Frame<Bytes>),
    Stream {
        receiver: mpsc::Receiver<RequestBodyPart>,
        polled: Option<oneshot::Sender<()>>,
    },
}

pub struct RawRequestBody {
    pub(crate) inner: RawRequestBodyInner,
}

impl RawRequestBody {
    pub(crate) fn new(body: AsyncRequestBody<'_, BodyWriter>) -> (RawRequestBody, Writer<'_>) {
        match body {
            AsyncRequestBody::Empty => (
                RawRequestBody {
                    inner: RawRequestBodyInner::Empty,
                },
                Writer::Nop,
            ),
            AsyncRequestBody::Fixed(body) => (
                RawRequestBody {
                    inner: RawRequestBodyInner::Single(Frame::data(body)),
                },
                Writer::Nop,
            ),
            AsyncRequestBody::Streaming(body) => {
                let (body_sender, body_receiver) = mpsc::channel(1);
                let (polled_sender, polled_receiver) = oneshot::channel();
                (
                    RawRequestBody {
                        inner: RawRequestBodyInner::Stream {
                            receiver: body_receiver,
                            polled: Some(polled_sender),
                        },
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

impl http_body::Body for RawRequestBody {
    type Data = Bytes;
    type Error = RequestBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match mem::replace(&mut self.inner, RawRequestBodyInner::Empty) {
            RawRequestBodyInner::Empty => Poll::Ready(None),
            RawRequestBodyInner::Single(frame) => Poll::Ready(Some(Ok(frame))),
            RawRequestBodyInner::Stream {
                mut receiver,
                mut polled,
            } => {
                if let Some(polled) = polled.take() {
                    let _ = polled.send(());
                }

                match Pin::new(&mut receiver).poll_next(cx) {
                    Poll::Ready(Some(RequestBodyPart::Frame(frame))) => {
                        self.inner = RawRequestBodyInner::Stream { receiver, polled };
                        Poll::Ready(Some(Ok(frame)))
                    }
                    Poll::Ready(Some(RequestBodyPart::Done)) => Poll::Ready(None),
                    Poll::Ready(None) => Poll::Ready(Some(Err(RequestBodyError(())))),
                    Poll::Pending => {
                        self.inner = RawRequestBodyInner::Stream { receiver, polled };
                        Poll::Pending
                    }
                }
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self.inner, RawRequestBodyInner::Empty)
    }

    fn size_hint(&self) -> SizeHint {
        match &self.inner {
            RawRequestBodyInner::Empty => SizeHint::with_exact(0),
            RawRequestBodyInner::Single(frame) => {
                let len = match frame.data_ref() {
                    Some(buf) => buf.len(),
                    None => 0,
                };
                SizeHint::with_exact(len as u64)
            }
            RawRequestBodyInner::Stream { .. } => SizeHint::new(),
        }
    }
}

pub(crate) enum Writer<'a> {
    Nop,
    Streaming {
        polled: oneshot::Receiver<()>,
        body: BoxAsyncWriteBody<'a, BodyWriter>,
        sender: mpsc::Sender<RequestBodyPart>,
    },
}

impl Writer<'_> {
    pub async fn write(self) -> Result<(), Error> {
        match self {
            Writer::Nop => Ok(()),
            Writer::Streaming {
                polled,
                mut body,
                sender,
            } => {
                // wait for hyper to actually ask for the body so we don't start reading it if the request fails early
                if polled.await.is_err() {
                    debug!("hyper hung up before polling request body");
                    return Ok(());
                }

                let writer = BodyWriter::new(sender);
                pin_mut!(writer);
                Pin::new(&mut body).write_body(writer.as_mut()).await?;
                writer.finish().await.map_err(Error::internal_safe)?;

                Ok(())
            }
        }
    }
}
