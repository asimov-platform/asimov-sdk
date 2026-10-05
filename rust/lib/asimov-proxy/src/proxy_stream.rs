// This is free and unencumbered software released into the public domain.

use alloc::boxed::Box;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use hyper_util::{
    client::legacy::connect::{Connected, Connection},
    rt::TokioIo,
};
use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

/// The stream types produced by the different `ProxyConfig` variants,
/// unified behind a trait object.
pub(crate) trait AsyncStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncStream for T {}

/// An established (possibly tunneled) connection to the target.
pub(crate) struct ProxyStream {
    io: TokioIo<Box<dyn AsyncStream>>,
}

impl ProxyStream {
    pub fn new(stream: impl AsyncStream + 'static) -> Self {
        Self {
            io: TokioIo::new(Box::new(stream)),
        }
    }
}

impl Connection for ProxyStream {
    fn connected(&self) -> Connected {
        // Note: even when proxied, the connection is always a tunnel carrying
        // end-to-end TLS, so `Connected::proxy(true)` (which switches hyper to
        // absolute-form request URIs) must *not* be set here.
        Connected::new()
    }
}

impl hyper::rt::Read for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl hyper::rt::Write for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
