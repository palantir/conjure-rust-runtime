use std::{
    io::{self, IoSlice, IoSliceMut},
    pin::Pin,
    task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite};
use pin_project::pin_project;
use tokio_rustls::client::TlsStream;

#[pin_project(project = MaybeHttpsStreamProj)]
pub enum MaybeHttpsStream<T> {
    Http(#[pin] T),
    Https(#[pin] TlsStream<T>),
}

impl<T> AsyncRead for MaybeHttpsStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_read(cx, buf),
            MaybeHttpsStreamProj::Https(s) => s.poll_read(cx, buf),
        }
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_read_vectored(cx, bufs),
            MaybeHttpsStreamProj::Https(s) => s.poll_read_vectored(cx, bufs),
        }
    }
}

impl<T> AsyncWrite for MaybeHttpsStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_write(cx, buf),
            MaybeHttpsStreamProj::Https(s) => s.poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_flush(cx),
            MaybeHttpsStreamProj::Https(s) => s.poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_close(cx),
            MaybeHttpsStreamProj::Https(s) => s.poll_close(cx),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            MaybeHttpsStreamProj::Http(s) => s.poll_write_vectored(cx, bufs),
            MaybeHttpsStreamProj::Https(s) => s.poll_write_vectored(cx, bufs),
        }
    }
}
