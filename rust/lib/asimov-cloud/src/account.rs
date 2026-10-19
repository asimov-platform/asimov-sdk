// This is free and unencumbered software released into the public domain.

use crate::error::AccountBalanceError;
use alloc::{boxed::Box, format};
use asimov_credit::Credits;
#[cfg(not(target_family = "wasm"))]
use asimov_http::proxy::{OriginProxy, ProxyOptions};
use core::{error::Error, str::FromStr};
use derive_more::Display;

pub use asimov_id::{Handle, HandleError};

#[derive(Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("@{}", self.0)]
pub struct Account(pub(crate) Handle);

impl FromStr for Account {
    type Err = HandleError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(Self(input.parse()?))
    }
}

impl Account {
    pub async fn open(id: Handle) -> Result<Self, Box<dyn Error>> {
        Ok(Self(id))
    }

    /// Checks whether [`Self::balance`] is positive, using its transport policy.
    ///
    /// # Errors
    ///
    /// Propagates errors from [`Self::balance`].
    pub async fn has_credits(&self) -> Result<bool, AccountBalanceError> {
        Ok(self.balance().await? > Credits::ZERO)
    }

    /// Fetches this account's credit balance from `https://asimov.credit`.
    ///
    /// Native targets use shared environment/native proxy discovery from
    /// `asimov-http`. Redirects are rejected because the resolved proxy policy
    /// belongs to one origin. On WebAssembly, proxy and redirect policy belong
    /// to the Fetch host.
    ///
    /// # Errors
    ///
    /// Returns proxy discovery, client construction, request, or credit parsing
    /// errors. Responses other than HTTP 200 yield
    /// [`AccountBalanceError::UnexpectedResponse`].
    pub async fn balance(&self) -> Result<Credits, AccountBalanceError> {
        let url = format!("https://asimov.credit/{}/balance.txt", self.0);
        Self::fetch_balance(&url).await
    }

    async fn fetch_balance(url: &str) -> Result<Credits, AccountBalanceError> {
        let builder = reqwest::Client::builder().user_agent("ASIMOV.rs");
        #[cfg(not(target_family = "wasm"))]
        let url = reqwest::Url::parse(url)
            .map_err(|_| asimov_http::proxy::Error::InvalidOrigin("expected an absolute URL"))?;
        #[cfg(not(target_family = "wasm"))]
        let builder =
            OriginProxy::resolve(&url, &ProxyOptions::default())?.configure_reqwest(builder)?;
        let client = builder.build()?;

        // Native requests use the exact URL whose proxy policy was resolved.
        let response = client.get(url).send().await?;

        if response.status() != reqwest::StatusCode::OK {
            return Err(AccountBalanceError::UnexpectedResponse(response.status()));
        }

        let response_text = response.text().await?;

        Ok(response_text.parse::<Credits>()?)
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests;
