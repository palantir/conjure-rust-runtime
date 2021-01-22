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
use crate::Builder;
use futures::ready;
use hyper::client::connect::{Connected, Connection};
use pin_project::pin_project;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tower_layer::Layer;
use tower_service::Service;

/// A connector layer which wraps a stream in a `TimeoutStream`.
pub struct TimeoutLayer {
    read_timeout: Duration,
    write_timeout: Duration,
}

impl TimeoutLayer {
    pub fn new<T>(builder: &Builder<T>) -> TimeoutLayer {
        TimeoutLayer {
            read_timeout: builder.get_read_timeout(),
            write_timeout: builder.get_write_timeout(),
        }
    }
}

impl<S> Layer<S> for TimeoutLayer {
    type Service = TimeoutService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimeoutService {
            inner,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
        }
    }
}

#[derive(Clone)]
pub struct TimeoutService<S> {
    inner: S,
    read_timeout: Duration,
    write_timeout: Duration,
}

impl<S, R> Service<R> for TimeoutService<S>
where
    S: Service<R>,
    S::Response: AsyncRead + AsyncWrite + Unpin,
{
    type Response = TimeoutStream<S::Response>;
    type Error = S::Error;
    type Future = TimeoutFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        TimeoutFuture {
            future: self.inner.call(req),
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
        }
    }
}

#[pin_project]
pub struct TimeoutFuture<F> {
    #[pin]
    future: F,
    read_timeout: Duration,
    write_timeout: Duration,
}

impl<F, S, E> Future for TimeoutFuture<F>
where
    F: Future<Output = Result<S, E>>,
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Result<TimeoutStream<S>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let stream = ready!(this.future.poll(cx))?;
        let mut stream = tokio_io_timeout::TimeoutStream::new(stream);
        stream.set_read_timeout(Some(*this.read_timeout));
        stream.set_write_timeout(Some(*this.write_timeout));

        Poll::Ready(Ok(TimeoutStream {
            stream: Box::pin(stream),
        }))
    }
}

#[derive(Debug)]
pub struct TimeoutStream<S> {
    stream: Pin<Box<tokio_io_timeout::TimeoutStream<S>>>,
}

impl<S> AsyncRead for TimeoutStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.stream.as_mut().poll_read(cx, buf)
    }
}

impl<S> AsyncWrite for TimeoutStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.stream.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.as_mut().poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.stream.as_mut().poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}

impl<S> Connection for TimeoutStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Connection,
{
    fn connected(&self) -> Connected {
        self.stream.get_ref().connected()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Client;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time;

    #[tokio::test]
    async fn read_no_timeout() {
        time::pause();

        let mut service =
            TimeoutLayer::new(&Client::builder()).layer(tower_util::service_fn(|_| async {
                Ok::<_, ()>(tokio_test::io::Builder::new().read(b"hello").build())
            }));

        let mut stream = service.call(()).await.unwrap();
        let mut buf = vec![];
        stream.read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, b"hello");
    }

    #[tokio::test]
    async fn read_timeout() {
        time::pause();

        let mut service = TimeoutLayer::new(Client::builder().read_timeout(Duration::from_secs(9)))
            .layer(tower_util::service_fn(|_| async {
                Ok::<_, ()>(
                    tokio_test::io::Builder::new()
                        .wait(Duration::from_secs(10))
                        .build(),
                )
            }));

        let mut stream = service.call(()).await.unwrap();
        let mut buf = vec![];
        stream.read_to_end(&mut buf).await.unwrap_err();
    }

    #[tokio::test]
    async fn write_no_timeout() {
        time::pause();

        let mut service =
            TimeoutLayer::new(&Client::builder()).layer(tower_util::service_fn(|_| async {
                Ok::<_, ()>(tokio_test::io::Builder::new().write(b"hello").build())
            }));

        let mut stream = service.call(()).await.unwrap();
        stream.write_all(b"hello").await.unwrap();
    }

    #[tokio::test]
    async fn write_timeout() {
        time::pause();

        let mut service = TimeoutLayer::new(
            Client::builder().write_timeout(Duration::from_secs(9)),
        )
        .layer(tower_util::service_fn(|_| async {
            Ok::<_, ()>(
                tokio_test::io::Builder::new()
                    .wait(Duration::from_secs(10))
                    .build(),
            )
        }));

        let mut stream = service.call(()).await.unwrap();
        stream.write_all(b"hello").await.unwrap_err();
    }
}
