// This is free and unencumbered software released into the public domain.

use alloc::string::String;
use core::str::FromStr;
use derive_more::{Debug, Display, From, FromStrError};

#[derive(Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum SocialHandle {
    #[cfg(feature = "facebook")]
    #[debug("SocialHandle::Facebook({:?})", _0.as_ref())]
    #[display("https://facebook.com/{_0}")]
    Facebook(crate::facebook::FacebookHandle),

    #[cfg(feature = "github")]
    #[debug("SocialHandle::Github({:?})", _0.as_ref())]
    #[display("https://github.com/{_0}")]
    Github(crate::github::GithubHandle),

    #[cfg(feature = "gravatar")]
    #[debug("SocialHandle::Gravatar({:?})", _0.as_ref())]
    #[display("https://gravatar.com/{_0}")]
    Gravatar(crate::gravatar::GravatarHandle),

    #[cfg(feature = "instagram")]
    #[debug("SocialHandle::Instagram({:?})", _0.as_ref())]
    #[display("https://instagram.com/{_0}")]
    Instagram(crate::instagram::InstagramHandle),

    #[cfg(feature = "introco")]
    #[debug("SocialHandle::Introco({:?})", _0.as_ref())]
    #[display("https://intro.co/{_0}")]
    Introco(crate::introco::IntrocoHandle),

    #[cfg(feature = "linkedin")]
    #[debug("SocialHandle::Linkedin({:?})", _0.as_ref())]
    #[display("https://linkedin.com/in/{_0}")]
    Linkedin(crate::linkedin::LinkedinHandle),

    #[cfg(feature = "localai")]
    #[debug("SocialHandle::Localai({:?})", _0.as_ref())]
    #[display("https://local.ai/{_0}")]
    Localai(crate::localai::LocalaiHandle),

    #[cfg(feature = "luma")]
    #[debug("SocialHandle::Luma({:?})", _0.as_ref())]
    #[display("https://luma.com/user/{_0}")]
    Luma(crate::luma::LumaHandle),

    #[cfg(feature = "telegram")]
    #[debug("SocialHandle::Telegram({:?})", _0.as_ref())]
    #[display("https://t.me/{_0}")]
    Telegram(crate::telegram::TelegramHandle),

    #[cfg(feature = "whatsapp")]
    #[debug("SocialHandle::Whatsapp({:?})", _0.as_ref())]
    #[display("https://wa.me/{_0}")]
    Whatsapp(crate::whatsapp::WhatsappHandle),

    #[cfg(feature = "x")]
    #[debug("SocialHandle::X({:?})", _0.as_ref())]
    #[display("https://x.com/{_0}")]
    X(crate::x::XHandle),
}

#[allow(unused)]
impl SocialHandle {
    #[cfg(feature = "facebook")]
    pub fn facebook(handle: impl Into<String>) -> Self {
        Self::Facebook(crate::facebook::FacebookHandle::from(handle.into()))
    }

    #[cfg(feature = "github")]
    pub fn github(handle: impl Into<String>) -> Self {
        Self::Github(crate::github::GithubHandle::from(handle.into()))
    }

    #[cfg(feature = "gravatar")]
    pub fn gravatar(handle: impl Into<String>) -> Self {
        Self::Gravatar(crate::gravatar::GravatarHandle::from(handle.into()))
    }

    #[cfg(feature = "instagram")]
    pub fn instagram(handle: impl Into<String>) -> Self {
        Self::Instagram(crate::instagram::InstagramHandle::from(handle.into()))
    }

    #[cfg(feature = "introco")]
    pub fn introco(handle: impl Into<String>) -> Self {
        Self::Introco(crate::introco::IntrocoHandle::from(handle.into()))
    }

    #[cfg(feature = "linkedin")]
    pub fn linkedin(handle: impl Into<String>) -> Self {
        Self::Linkedin(crate::linkedin::LinkedinHandle::from(handle.into()))
    }

    #[cfg(feature = "localai")]
    pub fn localai(handle: impl Into<String>) -> Self {
        Self::Localai(crate::localai::LocalaiHandle::from(handle.into()))
    }

    #[cfg(feature = "luma")]
    pub fn luma(handle: impl Into<String>) -> Self {
        Self::Luma(crate::luma::LumaHandle::from(handle.into()))
    }

    #[cfg(feature = "telegram")]
    pub fn telegram(handle: impl Into<String>) -> Self {
        Self::Telegram(crate::telegram::TelegramHandle::from(handle.into()))
    }

    #[cfg(feature = "whatsapp")]
    pub fn whatsapp(handle: impl Into<String>) -> Self {
        Self::Whatsapp(crate::whatsapp::WhatsappHandle::from(handle.into()))
    }

    #[cfg(feature = "x")]
    pub fn x(handle: impl Into<String>) -> Self {
        Self::X(crate::x::XHandle::from(handle.into()))
    }

    pub fn as_str(&self) -> &str {
        use SocialHandle::*;
        match self {
            #[cfg(feature = "facebook")]
            Facebook(h) => h.as_ref(),
            #[cfg(feature = "github")]
            Github(h) => h.as_ref(),
            #[cfg(feature = "gravatar")]
            Gravatar(h) => h.as_ref(),
            #[cfg(feature = "instagram")]
            Instagram(h) => h.as_ref(),
            #[cfg(feature = "introco")]
            Introco(h) => h.as_ref(),
            #[cfg(feature = "linkedin")]
            Linkedin(h) => h.as_ref(),
            #[cfg(feature = "localai")]
            Localai(h) => h.as_ref(),
            #[cfg(feature = "luma")]
            Luma(h) => h.as_ref(),
            #[cfg(feature = "telegram")]
            Telegram(h) => h.as_ref(),
            #[cfg(feature = "whatsapp")]
            Whatsapp(h) => h.as_ref(),
            #[cfg(feature = "x")]
            X(h) => h.as_ref(),
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
