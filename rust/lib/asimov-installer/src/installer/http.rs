// This is free and unencumbered software released into the public domain.

//! Installer GET/HEAD routing. Proxy decisions belong to `asimov-http`; the
//! installer owns client pools, redirect limits, and download timeouts.

use super::error::HttpError;
use alloc::{collections::VecDeque, sync::Arc};
use asimov_http::proxy::{OriginProxy, ProxyOptions};
use core::{fmt, time::Duration};
use reqwest::{Method, Response, StatusCode, Url, header::LOCATION};
use std::sync::Mutex;

const MAX_ORIGINS: usize = 16;
const MAX_REDIRECTS: usize = 10;

#[derive(Clone)]
pub(super) struct Client {
    mode: Mode,
}

#[derive(Clone)]
enum Mode {
    Automatic(Arc<Mutex<VecDeque<OriginClient>>>),
    Custom(reqwest::Client),
}

struct OriginClient {
    proxy: OriginProxy,
    client: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            mode: Mode::Automatic(Arc::new(Mutex::new(VecDeque::new()))),
        }
    }
}

impl From<reqwest::Client> for Client {
    fn from(client: reqwest::Client) -> Self {
        Self {
            mode: Mode::Custom(client),
        }
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Client settings may contain proxy credentials or service headers.
        f.write_str(match &self.mode {
            Mode::Automatic(_) => "AutomaticHttpClient",
            Mode::Custom(_) => "CustomHttpClient",
        })
    }
}

impl Client {
    pub(super) async fn get(&self, url: &str) -> Result<Response, HttpError> {
        self.send(Method::GET, url).await
    }

    pub(super) async fn head(&self, url: &str) -> Result<Response, HttpError> {
        self.send(Method::HEAD, url).await
    }

    async fn send(&self, method: Method, url: &str) -> Result<Response, HttpError> {
        let origins = match &self.mode {
            Mode::Custom(client) => return Ok(client.request(method, url).send().await?),
            Mode::Automatic(origins) => origins,
        };
        let mut url = Url::parse(url)
            .map_err(|_| asimov_http::proxy::Error::InvalidOrigin("expected an absolute URL"))?;
        let mut redirects = 0;
        loop {
            let client = Self::client_for(origins, &url)?;
            let response = client.request(method.clone(), url.clone()).send().await?;
            if !matches!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            ) {
                return Ok(response);
            }
            if redirects == MAX_REDIRECTS {
                return Err(HttpError::Redirect("too many redirects"));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .ok_or(HttpError::Redirect("missing Location header"))?
                .to_str()
                .map_err(|_| HttpError::Redirect("invalid Location header"))?;
            let next = url
                .join(location)
                .map_err(|_| HttpError::Redirect("invalid target URL"))?;
            // Discard redirect bodies without buffering or waiting for EOF.
            // Each bodyless GET/HEAD is rebuilt for its own resolved origin.
            drop(response);
            url = next;
            redirects += 1;
        }
    }

    fn client_for(
        origins: &Mutex<VecDeque<OriginClient>>,
        url: &Url,
    ) -> Result<reqwest::Client, HttpError> {
        // Only synchronous discovery/building uses this lock; requests and
        // response streams never hold it. The cache is safe to recover after
        // poisoning: incomplete client construction does not publish an entry.
        let mut origins = origins.lock().unwrap_or_else(|error| error.into_inner());
        let origin = url.origin();
        if let Some(index) = origins
            .iter()
            .position(|entry| entry.proxy.origin() == &origin)
        {
            let entry = origins.remove(index).expect("cached origin was found");
            let client = entry.client.clone();
            origins.push_back(entry);
            return Ok(client);
        }

        let proxy = OriginProxy::resolve(url, &ProxyOptions::default())?;
        // TODO: retry support
        let builder = reqwest::Client::builder()
            .user_agent("asimov-module-installer")
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30));
        let client = proxy.configure_reqwest(builder)?.build()?;
        if origins.len() == MAX_ORIGINS {
            origins.pop_front();
        }
        origins.push_back(OriginClient {
            proxy,
            client: client.clone(),
        });
        Ok(client)
    }
}

#[cfg(test)]
mod tests;
