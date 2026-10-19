// This is free and unencumbered software released into the public domain.

use crate::{HuggingfaceError as Error, Result};
use alloc::{collections::VecDeque, format};
use asimov_http::proxy::{OriginProxy, ProxyOptions};
use core::time::Duration;
use hf_hub::{Cache, Repo};
use reqwest::{
    Response, StatusCode, Url,
    header::{HeaderMap, HeaderValue},
};

pub(crate) struct Client {
    pub endpoint: Url,
    authorization: Option<HeaderValue>,
    origins: VecDeque<(OriginProxy, reqwest::Client)>,
}

impl Client {
    pub fn new(cache: &Cache) -> Result<Self> {
        let endpoint =
            std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".into());
        let endpoint = Url::parse(&format!("{}/", endpoint.trim_end_matches('/')))
            .map_err(|_| Error::InvalidInput("invalid HF_ENDPOINT URL"))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::InvalidInput(
                "HF_ENDPOINT must be an HTTP(S) base URL",
            ));
        }
        let authorization = cache
            .token()
            .map(|token| {
                let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| Error::InvalidInput("invalid saved token"))?;
                value.set_sensitive(true);
                Ok::<_, Error>(value)
            })
            .transpose()?;
        Ok(Self {
            endpoint,
            authorization,
            origins: VecDeque::new(),
        })
    }

    pub fn url(&self, repo: &Repo, filename: Option<&str>) -> Url {
        let mut url = self.endpoint.clone();
        let mut path = url
            .path_segments_mut()
            .expect("HTTP base URL has path segments");
        path.pop_if_empty();
        if filename.is_none() {
            path.extend(["api", "models"]);
        }
        path.extend(repo.url().split('/'));
        path.push(if filename.is_some() {
            "resolve"
        } else {
            "revision"
        });
        path.push(repo.revision());
        if let Some(filename) = filename {
            path.extend(filename.split('/'));
        }
        drop(path);
        url
    }

    pub async fn get(
        &mut self,
        mut url: Url,
        range: Option<&str>,
        mut observe: impl FnMut(&Url, &HeaderMap) -> Result<()>,
    ) -> Result<Response> {
        let mut redirects = 0;
        loop {
            if !url.username().is_empty() || url.password().is_some() {
                return Err(Error::InvalidResponse("redirect URL contains userinfo"));
            }
            let client = self.client_for(&url)?;
            let mut request = client
                .get(url.clone())
                .header("accept-encoding", "identity");
            if let Some(range) = range {
                request = request.header("range", range);
            }
            if url.origin() == self.endpoint.origin() {
                if let Some(token) = &self.authorization {
                    request = request.header("authorization", token.clone());
                }
            }
            let response = request.send().await?;
            observe(&url, response.headers())?;
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
            if redirects == 10 {
                return Err(Error::InvalidResponse("too many redirects"));
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or(Error::InvalidResponse(
                    "missing or invalid redirect Location",
                ))?;
            url = url
                .join(location)
                .map_err(|_| Error::InvalidResponse("invalid redirect URL"))?;
            drop(response);
            redirects += 1;
        }
    }

    fn client_for(&mut self, url: &Url) -> Result<reqwest::Client> {
        let origin = url.origin();
        if let Some(index) = self
            .origins
            .iter()
            .position(|(proxy, _)| proxy.origin() == &origin)
        {
            let entry = self.origins.remove(index).expect("cached origin was found");
            let client = entry.1.clone();
            self.origins.push_back(entry);
            return Ok(client);
        }
        let proxy = OriginProxy::resolve(url, &ProxyOptions::default())?;
        let builder = reqwest::Client::builder()
            .user_agent("ASIMOV.rs")
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();
        let client = proxy.configure_reqwest(builder)?.build()?;
        if self.origins.len() == 16 {
            self.origins.pop_front();
        }
        self.origins.push_back((proxy, client.clone()));
        Ok(client)
    }
}
