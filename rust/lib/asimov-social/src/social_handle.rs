// This is free and unencumbered software released into the public domain.

use alloc::string::{String, ToString};
use core::str::FromStr;
use derive_more::{Debug, Display, From, FromStrError};
use known_types::handle::ParseHandleError;

/// An account handle paired with the social platform to which it belongs.
///
/// Each variant is available only when its corresponding platform feature is
/// enabled. Platform-specific constructors parse a handle using the underlying
/// handle type; [`as_str`](Self::as_str) borrows that handle, while [`Display`]
/// formats it as an HTTPS profile or contact URL.
///
/// Parsing with [`FromStr`] (or converting from a [`String`] with `TryFrom`)
/// requires one of the exact URL prefixes documented on the variants. Trailing
/// `/` and `#` characters are removed before parsing. Bare handles, alternative
/// domains, and URLs for disabled platforms are not accepted by this parser.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github")]
/// # {
/// use asimov_social::SocialHandle;
///
/// let handle = SocialHandle::github("octocat")?;
/// assert_eq!(handle.as_str(), "octocat");
/// assert_eq!(handle.to_string(), "https://github.com/octocat");
/// assert_eq!("https://github.com/octocat/".parse::<SocialHandle>()?, handle);
/// # }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum SocialHandle {
    /// A Facebook handle, with URL prefix `https://facebook.com/`.
    #[cfg(feature = "facebook")]
    #[debug("SocialHandle::Facebook({:?})", _0.as_str())]
    #[display("https://facebook.com/{_0}")]
    Facebook(crate::facebook::FacebookHandle),

    /// A GitHub handle, with URL prefix `https://github.com/`.
    #[cfg(feature = "github")]
    #[debug("SocialHandle::Github({:?})", _0.as_str())]
    #[display("https://github.com/{_0}")]
    Github(crate::github::GithubHandle),

    /// A Gravatar handle, with URL prefix `https://gravatar.com/`.
    #[cfg(feature = "gravatar")]
    #[debug("SocialHandle::Gravatar({:?})", _0.as_str())]
    #[display("https://gravatar.com/{_0}")]
    Gravatar(crate::gravatar::GravatarHandle),

    /// An Instagram handle, with URL prefix `https://instagram.com/`.
    #[cfg(feature = "instagram")]
    #[debug("SocialHandle::Instagram({:?})", _0.as_str())]
    #[display("https://instagram.com/{_0}")]
    Instagram(crate::instagram::InstagramHandle),

    /// An Intro.co handle, with URL prefix `https://intro.co/`.
    #[cfg(feature = "introco")]
    #[debug("SocialHandle::Introco({:?})", _0.as_str())]
    #[display("https://intro.co/{_0}")]
    Introco(crate::introco::IntrocoHandle),

    /// A LinkedIn handle, with URL prefix `https://linkedin.com/in/`.
    #[cfg(feature = "linkedin")]
    #[debug("SocialHandle::Linkedin({:?})", _0.as_str())]
    #[display("https://linkedin.com/in/{_0}")]
    Linkedin(crate::linkedin::LinkedinHandle),

    /// A local.ai handle, with URL prefix `https://local.ai/`.
    #[cfg(feature = "localai")]
    #[debug("SocialHandle::Localai({:?})", _0.as_str())]
    #[display("https://local.ai/{_0}")]
    Localai(crate::localai::LocalaiHandle),

    /// A Luma handle, with URL prefix `https://luma.com/user/`.
    #[cfg(feature = "luma")]
    #[debug("SocialHandle::Luma({:?})", _0.as_str())]
    #[display("https://luma.com/user/{_0}")]
    Luma(crate::luma::LumaHandle),

    /// A Telegram handle, with URL prefix `https://t.me/`.
    #[cfg(feature = "telegram")]
    #[debug("SocialHandle::Telegram({:?})", _0.as_str())]
    #[display("https://t.me/{_0}")]
    Telegram(crate::telegram::TelegramHandle),

    /// A WhatsApp handle, with URL prefix `https://wa.me/`.
    #[cfg(feature = "whatsapp")]
    #[debug("SocialHandle::Whatsapp({:?})", _0.as_str())]
    #[display("https://wa.me/{_0}")]
    Whatsapp(crate::whatsapp::WhatsappHandle),

    /// An X handle, with URL prefix `https://x.com/`.
    #[cfg(feature = "x")]
    #[debug("SocialHandle::X({:?})", _0.as_str())]
    #[display("https://x.com/{_0}")]
    X(crate::x::XHandle),
}

#[cfg(feature = "async-graphql")]
impl async_graphql::connection::CursorType for SocialHandle {
    type Error = ParseHandleError;

    fn decode_cursor(input: &str) -> Result<Self, Self::Error> {
        Self::x(input) // FIXME
    }

    fn encode_cursor(&self) -> String {
        self.as_str().to_string()
    }
}

#[allow(unused)]
impl SocialHandle {
    /// Parses a Facebook handle, returning its parser's error on invalid input.
    #[cfg(feature = "facebook")]
    pub fn facebook(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::facebook::FacebookHandle;
        Ok(Self::Facebook(FacebookHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "github")]
    /// Parses a GitHub handle, returning its parser's error on invalid input.
    pub fn github(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::github::GithubHandle;
        Ok(Self::Github(GithubHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "gravatar")]
    /// Parses a Gravatar handle, returning its parser's error on invalid input.
    pub fn gravatar(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::gravatar::GravatarHandle;
        Ok(Self::Gravatar(GravatarHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "instagram")]
    /// Parses an Instagram handle, returning its parser's error on invalid input.
    pub fn instagram(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::instagram::InstagramHandle;
        Ok(Self::Instagram(InstagramHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "introco")]
    /// Parses an Intro.co handle, returning its parser's error on invalid input.
    pub fn introco(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::introco::IntrocoHandle;
        Ok(Self::Introco(IntrocoHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "linkedin")]
    /// Parses a LinkedIn handle, returning its parser's error on invalid input.
    pub fn linkedin(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::linkedin::LinkedinHandle;
        Ok(Self::Linkedin(LinkedinHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "localai")]
    /// Parses a local.ai handle, returning its parser's error on invalid input.
    pub fn localai(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::localai::LocalaiHandle;
        Ok(Self::Localai(LocalaiHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "luma")]
    /// Parses a Luma handle, returning its parser's error on invalid input.
    pub fn luma(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::luma::LumaHandle;
        Ok(Self::Luma(LumaHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "telegram")]
    /// Parses a Telegram handle, returning its parser's error on invalid input.
    pub fn telegram(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::telegram::TelegramHandle;
        Ok(Self::Telegram(TelegramHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "whatsapp")]
    /// Parses a WhatsApp handle, returning its parser's error on invalid input.
    pub fn whatsapp(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::whatsapp::WhatsappHandle;
        Ok(Self::Whatsapp(WhatsappHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "x")]
    /// Parses an X handle, returning its parser's error on invalid input.
    pub fn x(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::x::XHandle;
        Ok(Self::X(XHandle::from_str(input.as_ref())?))
    }

    /// Borrows the underlying handle without adding the platform's URL prefix.
    ///
    /// Use [`Display`] to format a URL that also identifies the platform.
    pub fn as_str(&self) -> &str {
        use SocialHandle::*;
        match self {
            #[cfg(feature = "facebook")]
            Facebook(h) => h.as_str(),
            #[cfg(feature = "github")]
            Github(h) => h.as_str(),
            #[cfg(feature = "gravatar")]
            Gravatar(h) => h.as_str(),
            #[cfg(feature = "instagram")]
            Instagram(h) => h.as_str(),
            #[cfg(feature = "introco")]
            Introco(h) => h.as_str(),
            #[cfg(feature = "linkedin")]
            Linkedin(h) => h.as_str(),
            #[cfg(feature = "localai")]
            Localai(h) => h.as_str(),
            #[cfg(feature = "luma")]
            Luma(h) => h.as_str(),
            #[cfg(feature = "telegram")]
            Telegram(h) => h.as_str(),
            #[cfg(feature = "whatsapp")]
            Whatsapp(h) => h.as_str(),
            #[cfg(feature = "x")]
            X(h) => h.as_str(),
        }
    }
}

impl FromStr for SocialHandle {
    type Err = FromStrError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim_end_matches(&['/', '#']);

        #[cfg(feature = "facebook")]
        if let Some(handle) = input.strip_prefix("https://facebook.com/") {
            return crate::facebook::FacebookHandle::from_str(handle)
                .map(Self::Facebook)
                .map_err(|_| FromStrError::new("Facebook"));
        }

        #[cfg(feature = "github")]
        if let Some(handle) = input.strip_prefix("https://github.com/") {
            return crate::github::GithubHandle::from_str(handle)
                .map(Self::Github)
                .map_err(|_| FromStrError::new("Github"));
        }

        #[cfg(feature = "gravatar")]
        if let Some(handle) = input.strip_prefix("https://gravatar.com/") {
            return crate::gravatar::GravatarHandle::from_str(handle)
                .map(Self::Gravatar)
                .map_err(|_| FromStrError::new("Gravatar"));
        }

        #[cfg(feature = "instagram")]
        if let Some(handle) = input.strip_prefix("https://instagram.com/") {
            return crate::instagram::InstagramHandle::from_str(handle)
                .map(Self::Instagram)
                .map_err(|_| FromStrError::new("Instagram"));
        }

        #[cfg(feature = "introco")]
        if let Some(handle) = input.strip_prefix("https://intro.co/") {
            return crate::introco::IntrocoHandle::from_str(handle)
                .map(Self::Introco)
                .map_err(|_| FromStrError::new("Introco"));
        }

        #[cfg(feature = "linkedin")]
        if let Some(handle) = input.strip_prefix("https://linkedin.com/in/") {
            return crate::linkedin::LinkedinHandle::from_str(handle)
                .map(Self::Linkedin)
                .map_err(|_| FromStrError::new("Linkedin"));
        }

        #[cfg(feature = "localai")]
        if let Some(handle) = input.strip_prefix("https://local.ai/") {
            return crate::localai::LocalaiHandle::from_str(handle)
                .map(Self::Localai)
                .map_err(|_| FromStrError::new("Localai"));
        }

        #[cfg(feature = "luma")]
        if let Some(handle) = input.strip_prefix("https://luma.com/user/") {
            return crate::luma::LumaHandle::from_str(handle)
                .map(Self::Luma)
                .map_err(|_| FromStrError::new("Luma"));
        }

        #[cfg(feature = "telegram")]
        if let Some(handle) = input.strip_prefix("https://t.me/") {
            return crate::telegram::TelegramHandle::from_str(handle)
                .map(Self::Telegram)
                .map_err(|_| FromStrError::new("Telegram"));
        }

        #[cfg(feature = "whatsapp")]
        if let Some(handle) = input.strip_prefix("https://wa.me/") {
            return crate::whatsapp::WhatsappHandle::from_str(handle)
                .map(Self::Whatsapp)
                .map_err(|_| FromStrError::new("Whatsapp"));
        }

        #[cfg(feature = "x")]
        if let Some(handle) = input.strip_prefix("https://x.com/") {
            return crate::x::XHandle::from_str(handle)
                .map(Self::X)
                .map_err(|_| FromStrError::new("X"));
        }

        Err(FromStrError::new("Other"))
    }
}

impl TryFrom<String> for SocialHandle {
    type Error = <Self as FromStr>::Err;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        <Self as FromStr>::from_str(&input)
    }
}
