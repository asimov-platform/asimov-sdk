// This is free and unencumbered software released into the public domain.

use alloc::string::{String, ToString};
use core::str::FromStr;
use derive_more::{Debug, Display, From, FromStrError};
use known_types::handle::ParseHandleError;

#[derive(Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum SocialHandle {
    #[cfg(feature = "facebook")]
    #[debug("SocialHandle::Facebook({:?})", _0.as_str())]
    #[display("https://facebook.com/{_0}")]
    Facebook(crate::facebook::FacebookHandle),

    #[cfg(feature = "github")]
    #[debug("SocialHandle::Github({:?})", _0.as_str())]
    #[display("https://github.com/{_0}")]
    Github(crate::github::GithubHandle),

    #[cfg(feature = "gravatar")]
    #[debug("SocialHandle::Gravatar({:?})", _0.as_str())]
    #[display("https://gravatar.com/{_0}")]
    Gravatar(crate::gravatar::GravatarHandle),

    #[cfg(feature = "instagram")]
    #[debug("SocialHandle::Instagram({:?})", _0.as_str())]
    #[display("https://instagram.com/{_0}")]
    Instagram(crate::instagram::InstagramHandle),

    #[cfg(feature = "introco")]
    #[debug("SocialHandle::Introco({:?})", _0.as_str())]
    #[display("https://intro.co/{_0}")]
    Introco(crate::introco::IntrocoHandle),

    #[cfg(feature = "linkedin")]
    #[debug("SocialHandle::Linkedin({:?})", _0.as_str())]
    #[display("https://linkedin.com/in/{_0}")]
    Linkedin(crate::linkedin::LinkedinHandle),

    #[cfg(feature = "localai")]
    #[debug("SocialHandle::Localai({:?})", _0.as_str())]
    #[display("https://local.ai/{_0}")]
    Localai(crate::localai::LocalaiHandle),

    #[cfg(feature = "luma")]
    #[debug("SocialHandle::Luma({:?})", _0.as_str())]
    #[display("https://luma.com/user/{_0}")]
    Luma(crate::luma::LumaHandle),

    #[cfg(feature = "telegram")]
    #[debug("SocialHandle::Telegram({:?})", _0.as_str())]
    #[display("https://t.me/{_0}")]
    Telegram(crate::telegram::TelegramHandle),

    #[cfg(feature = "whatsapp")]
    #[debug("SocialHandle::Whatsapp({:?})", _0.as_str())]
    #[display("https://wa.me/{_0}")]
    Whatsapp(crate::whatsapp::WhatsappHandle),

    #[cfg(feature = "x")]
    #[debug("SocialHandle::X({:?})", _0.as_str())]
    #[display("https://x.com/{_0}")]
    X(crate::x::XHandle),
}

#[cfg(feature = "async-graphql")]
impl async_graphql::connection::CursorType for SocialHandle {
    type Error = core::convert::Infallible;

    fn decode_cursor(input: &str) -> Result<Self, Self::Error> {
        Ok(Self::x(input)) // FIXME
    }

    fn encode_cursor(&self) -> String {
        self.as_str().to_string()
    }
}

#[allow(unused)]
impl SocialHandle {
    #[cfg(feature = "facebook")]
    pub fn facebook(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::facebook::FacebookHandle;
        Ok(Self::Facebook(FacebookHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "github")]
    pub fn github(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::github::GithubHandle;
        Ok(Self::Github(GithubHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "gravatar")]
    pub fn gravatar(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::gravatar::GravatarHandle;
        Ok(Self::Gravatar(GravatarHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "instagram")]
    pub fn instagram(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::instagram::InstagramHandle;
        Ok(Self::Instagram(InstagramHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "introco")]
    pub fn introco(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::introco::IntrocoHandle;
        Ok(Self::Introco(IntrocoHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "linkedin")]
    pub fn linkedin(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::linkedin::LinkedinHandle;
        Ok(Self::Linkedin(LinkedinHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "localai")]
    pub fn localai(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::localai::LocalaiHandle;
        Ok(Self::Localai(LocalaiHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "luma")]
    pub fn luma(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::luma::LumaHandle;
        Ok(Self::Luma(LumaHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "telegram")]
    pub fn telegram(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::telegram::TelegramHandle;
        Ok(Self::Telegram(TelegramHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "whatsapp")]
    pub fn whatsapp(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::whatsapp::WhatsappHandle;
        Ok(Self::Whatsapp(WhatsappHandle::from_str(input.as_ref())?))
    }

    #[cfg(feature = "x")]
    pub fn x(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
        use crate::x::XHandle;
        Ok(Self::X(XHandle::from_str(input.as_ref())?))
    }

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
