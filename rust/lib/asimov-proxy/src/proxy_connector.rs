// This is free and unencumbered software released into the public domain.

//! Upstream connection establishment, optionally through a proxy.
//!
//! Receives one destination origin and resolves shared proxy policy at setup.

use super::{proxy_config::ProxyConfig, proxy_stream::ProxyStream};
use crate::{BoxError, Error, ProxyMode, UpstreamProxyOptions};
use alloc::{boxed::Box, format, string::ToString, sync::Arc, vec::Vec};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use hyper::Uri;
use rustls::pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use tokio_rustls::TlsConnector;
use tokio_socks::tcp::Socks5Stream;
use tower_service::Service;

/// A connector for the hyper client that establishes TCP connections either
/// directly or through the configured proxy.
///
/// TLS to the *target* is layered on top by `hyper_rustls::HttpsConnector`;
/// this connector only concerns itself with producing a raw byte stream to
/// the target (possibly tunneled, possibly itself TLS-wrapped in the case of
/// an `https://` proxy).
#[derive(Clone)]
pub(crate) struct ProxyConnector {
    config: Arc<ProxyConfig>,
    origin: url::Origin,
    /// TLS used for connecting to `https://` proxies (not the target).
    proxy_tls: TlsConnector,
}

impl ProxyConnector {
    pub fn new(
        target: &url::Url,
        config: ProxyConfig,
        tls_config: Arc<rustls::ClientConfig>,
    ) -> Result<Self, Error> {
        let scope = asimov_http::proxy::OriginProxy::resolve(
            target,
            &UpstreamProxyOptions {
                mode: ProxyMode::Direct,
                no_proxy: None,
            },
        )?;
        let config = config.for_target(target)?;
        Ok(Self {
            config: Arc::new(config),
            origin: scope.origin().clone(),
            proxy_tls: TlsConnector::from(tls_config),
        })
    }
}

impl Service<Uri> for ProxyConnector {
    type Response = ProxyStream;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let config = Arc::clone(&self.config);
        let origin = self.origin.clone();
        let proxy_tls = self.proxy_tls.clone();

        Box::pin(async move {
            if url::Url::parse(&dst.to_string())?.origin() != origin {
                return Err(asimov_http::proxy::Error::InvalidOrigin(
                    "connector must use its configured origin",
                )
                .into());
            }
            let target_host = dst
                .host()
                .ok_or("target URI is missing a host")?
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            let target_port = dst.port_u16().unwrap_or(match dst.scheme_str() {
                Some("https") => 443,
                _ => 80,
            });

            match &*config {
                ProxyConfig::Automatic => Err("proxy policy was not resolved at setup".into()),
                ProxyConfig::Direct => {
                    let stream = TcpStream::connect((target_host.as_str(), target_port)).await?;
                    Ok(ProxyStream::new(stream))
                },

                ProxyConfig::HttpConnect {
                    host,
                    port,
                    tls,
                    basic_auth,
                } => {
                    let stream = TcpStream::connect((host.as_str(), *port)).await?;
                    if *tls {
                        let server_name = ServerName::try_from(host.clone())?;
                        let stream = proxy_tls.connect(server_name, stream).await?;
                        let stream =
                            http_connect(stream, &target_host, target_port, basic_auth.as_deref())
                                .await?;
                        Ok(ProxyStream::new(stream))
                    } else {
                        let stream =
                            http_connect(stream, &target_host, target_port, basic_auth.as_deref())
                                .await?;
                        Ok(ProxyStream::new(stream))
                    }
                },

                ProxyConfig::Socks5 {
                    host,
                    port,
                    auth,
                    remote_dns,
                } => {
                    let proxy_addr = (host.as_str(), *port);
                    let stream = if *remote_dns {
                        let target = (target_host, target_port);
                        match auth {
                            Some((user, pass)) => {
                                Socks5Stream::connect_with_password(proxy_addr, target, user, pass)
                                    .await?
                            },
                            None => Socks5Stream::connect(proxy_addr, target).await?,
                        }
                    } else {
                        let target = tokio::net::lookup_host((target_host.as_str(), target_port))
                            .await?
                            .next()
                            .ok_or("failed to resolve the target host")?;
                        match auth {
                            Some((user, pass)) => {
                                Socks5Stream::connect_with_password(proxy_addr, target, user, pass)
                                    .await?
                            },
                            None => Socks5Stream::connect(proxy_addr, target).await?,
                        }
                    };
                    Ok(ProxyStream::new(stream))
                },
            }
        })
    }
}

/// Performs an HTTP/1.1 `CONNECT` handshake over `stream`, returning the
/// stream once the tunnel to `host:port` is established.
async fn http_connect<S>(
    mut stream: S,
    host: &str,
    port: u16,
    basic_auth: Option<&str>,
) -> Result<S, BoxError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(auth) = basic_auth {
        request.push_str(&format!("Proxy-Authorization: Basic {}\r\n", auth));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::with_capacity(1024);
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() == 8192 {
            return Err("oversized CONNECT response from the proxy".into());
        }
        // Stop exactly at the header boundary. Tunnel bytes may arrive in the
        // same packet and must remain available to the upstream TLS/HTTP layer.
        response.push(stream.read_u8().await?);
    }

    let mut status_line = response
        .split(|byte| *byte == b'\r')
        .next()
        .unwrap_or_default()
        .split(|byte| *byte == b' ');
    if !matches!(status_line.next(), Some(b"HTTP/1.0" | b"HTTP/1.1")) {
        return Err("invalid CONNECT response from the proxy".into());
    }
    let status = status_line
        .next()
        .and_then(|value| core::str::from_utf8(value).ok())
        .filter(|value| value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|status| (100..=599).contains(status))
        .ok_or("invalid CONNECT response status")?;
    if status != 200 {
        return Err(format!("the proxy refused CONNECT: HTTP {status}").into());
    }

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[tokio::test]
    async fn connect_tunnels_authenticate_and_format_ipv6_authorities() {
        let (client, mut proxy) = tokio::io::duplex(1024);
        let server = tokio::spawn(async move {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(proxy.read_u8().await.unwrap());
            }
            assert_eq!(
                String::from_utf8(request).unwrap(),
                "CONNECT [::1]:443 HTTP/1.1\r\nHost: [::1]:443\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n\r\n"
            );
            proxy
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n\x07")
                .await
                .unwrap();
            assert_eq!(proxy.read_u8().await.unwrap(), 42);
        });
        let mut tunnel = http_connect(client, "::1", 443, Some("dXNlcjpwYXNz"))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(core::time::Duration::from_secs(2), tunnel.read_u8())
                .await
                .unwrap()
                .unwrap(),
            7
        );
        tunnel.write_all(&[42]).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connect_rejects_invalid_and_oversized_headers_without_echoing_secrets() {
        for response in [
            "not-http 200 reason\r\n\r\n".into(),
            "HTTP/1.1 secret-status reason\r\n\r\n".into(),
            "HTTP/1.1 0200 reason\r\n\r\n".into(),
            "HTTP/1.1 407 secret-reason\r\n\r\n".into(),
            format!("HTTP/1.1 200 OK\r\nX-Fill: {}\r\n\r\n", "x".repeat(8192)),
        ] {
            let (client, mut proxy) = tokio::io::duplex(16384);
            let server = tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(proxy.read_u8().await.unwrap());
                }
                proxy.write_all(response.as_bytes()).await.unwrap();
            });
            let error = match http_connect(client, "service.invalid", 443, None).await {
                Err(error) => error,
                Ok(_) => panic!("invalid CONNECT reply was accepted"),
            };
            assert!(!error.to_string().contains("secret"));
            server.await.unwrap();
        }
    }
}
