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
use crate::{builder, Builder};
use futures::ready;
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::TokioIo;
use pin_project::pin_project;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower_layer::Layer;
use tower_service::Service;

/// A connector layer which wraps a stream in a `TimeoutStream`.
pub struct TimeoutLayer {
    read_timeout: Duration,
    write_timeout: Duration,
}

impl TimeoutLayer {
    pub fn new<T>(builder: &Builder<builder::Complete<T>>) -> TimeoutLayer {
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
    S::Response: Read + Write + Unpin,
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
    S: Read + Write + Unpin,
{
    type Output = Result<TimeoutStream<S>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let stream = ready!(this.future.poll(cx))?;
        let mut stream = tokio_io_timeout::TimeoutStream::new(TokioIo::new(stream));
        stream.set_read_timeout(Some(*this.read_timeout));
        stream.set_write_timeout(Some(*this.write_timeout));

        Poll::Ready(Ok(TimeoutStream {
            stream: Box::pin(TokioIo::new(stream)),
        }))
    }
}

#[derive(Debug)]
pub struct TimeoutStream<S> {
    stream: Pin<Box<TokioIo<tokio_io_timeout::TimeoutStream<TokioIo<S>>>>>,
}

impl<S> Read for TimeoutStream<S>
where
    S: Read + Write,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        self.stream.as_mut().poll_read(cx, buf)
    }
}

impl<S> Write for TimeoutStream<S>
where
    S: Read + Write,
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
}

impl<S> Connection for TimeoutStream<S>
where
    S: Read + Write + Connection,
{
    fn connected(&self) -> Connected {
        self.stream.inner().get_ref().inner().connected()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time;

    #[tokio::test]
    async fn read_no_timeout() {
        time::pause();

        let mut service =
            TimeoutLayer::new(&Builder::for_test()).layer(tower_util::service_fn(|_| async {
                Ok::<_, ()>(TokioIo::new(
                    tokio_test::io::Builder::new().read(b"hello").build(),
                ))
            }));

        let stream = service.call(()).await.unwrap();
        let mut buf = vec![];
        TokioIo::new(stream).read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, b"hello");
    }

    #[tokio::test]
    async fn read_timeout() {
        time::pause();

        let mut service = TimeoutLayer::new(
            &Builder::for_test().read_timeout(Duration::from_secs(9)),
        )
        .layer(tower_util::service_fn(|_| async {
            Ok::<_, ()>(TokioIo::new(
                tokio_test::io::Builder::new()
                    .wait(Duration::from_secs(10))
                    .build(),
            ))
        }));

        let stream = service.call(()).await.unwrap();
        let mut buf = vec![];
        TokioIo::new(stream)
            .read_to_end(&mut buf)
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn write_no_timeout() {
        time::pause();

        let mut service =
            TimeoutLayer::new(&Builder::for_test()).layer(tower_util::service_fn(|_| async {
                Ok::<_, ()>(TokioIo::new(
                    tokio_test::io::Builder::new().write(b"hello").build(),
                ))
            }));

        let stream = service.call(()).await.unwrap();
        TokioIo::new(stream).write_all(b"hello").await.unwrap();
    }

    #[tokio::test]
    async fn write_timeout() {
        time::pause();

        let mut service = TimeoutLayer::new(
            &Builder::for_test().write_timeout(Duration::from_secs(9)),
        )
        .layer(tower_util::service_fn(|_| async {
            Ok::<_, ()>(TokioIo::new(
                tokio_test::io::Builder::new()
                    .wait(Duration::from_secs(10))
                    .build(),
            ))
        }));

        let stream = service.call(()).await.unwrap();
        TokioIo::new(stream).write_all(b"hello").await.unwrap_err();
    }
}
