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
use crate::raw::{BodyPart, DefaultRawBody};
use crate::BaseBody;
use bytes::{Buf, Bytes, BytesMut};
use conjure_error::Error;
use futures::channel::mpsc;
use futures::{ready, SinkExt, Stream};
use http_body::{Body, Frame, SizeHint};
use pin_project::pin_project;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{error, io, mem};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

/// The asynchronous writer passed to
/// [`AsyncWriteBody::write_body()`](conjure_http::client::AsyncWriteBody::write_body()).
#[pin_project]
pub struct BodyWriter {
    #[pin]
    sender: mpsc::Sender<BodyPart>,
    buf: BytesMut,
    #[pin]
    _p: PhantomPinned,
}

impl BodyWriter {
    pub(crate) fn new(sender: mpsc::Sender<BodyPart>) -> BodyWriter {
        BodyWriter {
            sender,
            buf: BytesMut::new(),
            _p: PhantomPinned,
        }
    }

    pub(crate) async fn finish(mut self: Pin<&mut Self>) -> io::Result<()> {
        self.flush().await?;
        self.project()
            .sender
            .send(BodyPart::Done)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    /// Writes a block of body bytes.
    ///
    /// Compared to the [`AsyncWrite`] implementation, this method avoids some copies if the caller already has the body
    /// in [`Bytes`] objects.
    pub async fn write_bytes(mut self: Pin<&mut Self>, bytes: Bytes) -> io::Result<()> {
        self.flush().await?;
        self.project()
            .sender
            .send(BodyPart::Frame(Frame::data(bytes)))
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

        self.project().buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut this = self.project();

        if this.buf.is_empty() {
            return Poll::Ready(Ok(()));
        }

        ready!(this.sender.poll_ready(cx)).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let chunk = this.buf.split().freeze();
        this.sender
            .start_send(BodyPart::Frame(Frame::data(chunk)))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// An asynchronous streaming response body.
#[pin_project]
pub struct ResponseBody<B = DefaultRawBody> {
    #[pin]
    body: FuseBody<BaseBody<B>>,
    cur: Bytes,
    // Make sure we can make our internal BaseBody !Unpin in the future if we want
    #[pin]
    _p: PhantomPinned,
}

impl<B> ResponseBody<B> {
    pub(crate) fn new(body: BaseBody<B>) -> Self {
        ResponseBody {
            body: FuseBody::new(body),
            cur: Bytes::new(),
            _p: PhantomPinned,
        }
    }

    pub(crate) fn buffer(&self) -> &[u8] {
        &self.cur
    }
}

impl<B> Stream for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if this.cur.has_remaining() {
            return Poll::Ready(Some(Ok(mem::take(this.cur))));
        }

        loop {
            match ready!(this.body.as_mut().poll_frame(cx))
                .transpose()
                .map_err(Error::internal_safe)?
            {
                Some(frame) => {
                    if let Ok(data) = frame.into_data() {
                        return Poll::Ready(Some(Ok(data)));
                    }
                }
                None => return Poll::Ready(None),
            }
        }
    }
}

impl<B> AsyncRead for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let in_buf = ready!(self.as_mut().poll_fill_buf(cx))?;
        let len = usize::min(in_buf.len(), buf.remaining());
        buf.put_slice(&in_buf[..len]);
        self.consume(len);

        Poll::Ready(Ok(()))
    }
}

impl<B> AsyncBufRead for ResponseBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    fn poll_fill_buf(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        while !self.cur.has_remaining() {
            match ready!(self.as_mut().project().body.poll_frame(cx))
                .transpose()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            {
                Some(frame) => {
                    if let Ok(data) = frame.into_data() {
                        *self.as_mut().project().cur = data;
                    }
                }
                None => break,
            }
        }

        Poll::Ready(Ok(self.project().cur))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.project().cur.advance(amt)
    }
}

#[pin_project]
struct FuseBody<B> {
    #[pin]
    body: B,
    done: bool,
}

impl<B> FuseBody<B> {
    fn new(body: B) -> FuseBody<B> {
        FuseBody { body, done: false }
    }
}

impl<B> Body for FuseBody<B>
where
    B: Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();

        if *this.done {
            return Poll::Ready(None);
        }

        let frame = ready!(this.body.poll_frame(cx));
        if frame.is_none() {
            *this.done = true;
        }

        Poll::Ready(frame)
    }

    fn is_end_stream(&self) -> bool {
        self.done || self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        if self.done {
            SizeHint::with_exact(0)
        } else {
            self.body.size_hint()
        }
    }
}
