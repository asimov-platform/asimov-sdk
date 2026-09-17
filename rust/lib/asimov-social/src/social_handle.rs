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
/// uses [`url::Url`] to normalize scheme/host casing, default ports, and dot
/// segments. An optional `www.` hostname prefix is discarded. Trailing `/` and
/// `#` characters are removed before parsing. Credentials, non-default ports,
/// queries, and nonempty fragments are rejected. Reddit's `/u/` and Threads'
/// `threads.com` aliases are accepted. Bare handles, other domains, and URLs for
/// disabled platforms are rejected. Formatting always omits `www.`.
///
/// Fallible conversions to and from [`crate::SocialLink`] support plain profiles
/// on platforms shared by the two types. See its conversion documentation for
/// supported platforms and validation rules.
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
    /// A Bluesky domain handle; see [`crate::SocialLink::BlueskyProfile`].
    #[cfg(feature = "bluesky")]
    #[display("https://bsky.app/profile/{_0}")]
    Bluesky(crate::BlueskyHandle),

    /// A Discord numeric user ID; see [`crate::SocialLink::DiscordProfile`].
    #[cfg(feature = "discord")]
    #[display("https://discord.com/users/{_0}")]
    Discord(crate::DiscordHandle),

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

    /// A GitLab.com username; see [`crate::SocialLink::GitlabProfile`].
    #[cfg(feature = "gitlab")]
    #[display("https://gitlab.com/{_0}")]
    Gitlab(crate::GitlabHandle),

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
    #[display("https://linkedin.com/in/{_0}/")]
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

    /// A Medium handle; see [`crate::SocialLink::MediumProfile`].
    #[cfg(feature = "medium")]
    #[display("https://medium.com/@{_0}")]
    Medium(crate::MediumHandle),

    /// A Pinterest username; see [`crate::SocialLink::PinterestProfile`].
    #[cfg(feature = "pinterest")]
    #[display("https://pinterest.com/{_0}")]
    Pinterest(crate::PinterestHandle),

    /// A Reddit username; see [`crate::SocialLink::RedditProfile`].
    #[cfg(feature = "reddit")]
    #[display("https://reddit.com/user/{_0}")]
    Reddit(crate::RedditHandle),

    /// A Snapchat username; see [`crate::SocialLink::SnapchatProfile`].
    #[cfg(feature = "snapchat")]
    #[display("https://snapchat.com/add/{_0}")]
    Snapchat(crate::SnapchatHandle),

    /// A Substack profile handle; see [`crate::SocialLink::SubstackProfile`].
    #[cfg(feature = "substack")]
    #[display("https://substack.com/@{_0}")]
    Substack(crate::SubstackHandle),

    /// A Telegram handle, with URL prefix `https://t.me/`.
    #[cfg(feature = "telegram")]
    #[debug("SocialHandle::Telegram({:?})", _0.as_str())]
    #[display("https://t.me/{_0}")]
    Telegram(crate::telegram::TelegramHandle),

    /// A Threads username; see [`crate::SocialLink::ThreadsProfile`].
    #[cfg(feature = "threads")]
    #[display("https://threads.net/@{_0}")]
    Threads(crate::ThreadsHandle),

    /// A TikTok username; see [`crate::SocialLink::TiktokProfile`].
    #[cfg(feature = "tiktok")]
    #[display("https://tiktok.com/@{_0}")]
    Tiktok(crate::TiktokHandle),

    /// A Twitch username; see [`crate::SocialLink::TwitchProfile`].
    #[cfg(feature = "twitch")]
    #[display("https://twitch.tv/{_0}")]
    Twitch(crate::TwitchHandle),

    /// A WhatsApp handle, with URL prefix `https://wa.me/`.
    #[cfg(feature = "whatsapp")]
    #[debug("SocialHandle::Whatsapp({:?})", _0.as_str())]
    #[display("https://wa.me/{_0}")]
    Whatsapp(crate::whatsapp::WhatsappHandle),

    /// An X (fka Twitter) handle, with URL prefix `https://x.com/`.
    #[cfg(feature = "x")]
    #[debug("SocialHandle::X({:?})", _0.as_str())]
    #[display("https://x.com/{_0}")]
    X(crate::x::XHandle),

    /// A YouTube handle; see [`crate::SocialLink::YoutubeProfile`].
    #[cfg(feature = "youtube")]
    #[display("https://youtube.com/@{}", crate::platform_handles::encoded_handle(_0.as_str()))]
    Youtube(crate::YoutubeHandle),
}

#[cfg(feature = "async-graphql")]
/// Uses the canonical profile URL as a cursor, preserving the platform identity.
impl async_graphql::connection::CursorType for SocialHandle {
    type Error = FromStrError;

    fn decode_cursor(input: &str) -> Result<Self, Self::Error> {
        input.parse()
    }

    fn encode_cursor(&self) -> String {
        self.to_string()
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
            #[cfg(feature = "bluesky")]
            Bluesky(h) => h.as_str(),
            #[cfg(feature = "discord")]
            Discord(h) => h.as_str(),
            #[cfg(feature = "facebook")]
            Facebook(h) => h.as_str(),
            #[cfg(feature = "github")]
            Github(h) => h.as_str(),
            #[cfg(feature = "gitlab")]
            Gitlab(h) => h.as_str(),
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
            #[cfg(feature = "medium")]
            Medium(h) => h.as_str(),
            #[cfg(feature = "pinterest")]
            Pinterest(h) => h.as_str(),
            #[cfg(feature = "reddit")]
            Reddit(h) => h.as_str(),
            #[cfg(feature = "snapchat")]
            Snapchat(h) => h.as_str(),
            #[cfg(feature = "substack")]
            Substack(h) => h.as_str(),
            #[cfg(feature = "telegram")]
            Telegram(h) => h.as_str(),
            #[cfg(feature = "threads")]
            Threads(h) => h.as_str(),
            #[cfg(feature = "tiktok")]
            Tiktok(h) => h.as_str(),
            #[cfg(feature = "twitch")]
            Twitch(h) => h.as_str(),
            #[cfg(feature = "whatsapp")]
            Whatsapp(h) => h.as_str(),
            #[cfg(feature = "x")]
            X(h) => h.as_str(),
            #[cfg(feature = "youtube")]
            Youtube(h) => h.as_str(),
            // With every platform disabled, `&SocialHandle` is still considered
            // inhabited by the exhaustiveness checker, but cannot be constructed.
            #[allow(unreachable_patterns)]
            _ => unreachable!("a handle requires an enabled platform"),
        }
    }
}

impl FromStr for SocialHandle {
    type Err = FromStrError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim_end_matches(&['/', '#']);
        let url = crate::social_url::parse(input).map_err(|_| FromStrError::new("SocialHandle"))?;
        if url.query().is_some() || url.fragment().is_some() {
            return Err(FromStrError::new("SocialHandle"));
        }
        let host = url.host_str().ok_or(FromStrError::new("SocialHandle"))?;
        let platform = crate::SocialPlatform::from_host(host)?;
        let path = url.path().strip_prefix('/').unwrap_or_default();
        let prefix = if platform == crate::SocialPlatform::Reddit && path.starts_with("u/") {
            "u/"
        } else {
            platform.handle_prefix().unwrap_or("")
        };
        let identifier = path
            .strip_prefix(prefix)
            .filter(|id| !id.is_empty() && !id.contains(['/', '?', '#']))
            .ok_or(FromStrError::new("SocialHandle"))?;
        platform
            .handle(identifier)
            .map_err(|_| FromStrError::new("SocialHandle"))
    }
}

macro_rules! constructor {
    ($feature:literal, $method:ident, $variant:ident, $type:ident) => {
        #[cfg(feature = $feature)]
        impl SocialHandle {
            #[doc = concat!("Parses an identifier using [`crate::", stringify!($type), "`].")]
            pub fn $method(input: impl AsRef<str>) -> Result<Self, ParseHandleError> {
                input.as_ref().parse::<crate::$type>().map(Self::$variant)
            }
        }
    };
}

constructor!("bluesky", bluesky, Bluesky, BlueskyHandle);
constructor!("discord", discord, Discord, DiscordHandle);
constructor!("gitlab", gitlab, Gitlab, GitlabHandle);
constructor!("medium", medium, Medium, MediumHandle);
constructor!("pinterest", pinterest, Pinterest, PinterestHandle);
constructor!("reddit", reddit, Reddit, RedditHandle);
constructor!("snapchat", snapchat, Snapchat, SnapchatHandle);
constructor!("substack", substack, Substack, SubstackHandle);
constructor!("threads", threads, Threads, ThreadsHandle);
constructor!("tiktok", tiktok, Tiktok, TiktokHandle);
constructor!("twitch", twitch, Twitch, TwitchHandle);
constructor!("youtube", youtube, Youtube, YoutubeHandle);

impl TryFrom<String> for SocialHandle {
    type Error = <Self as FromStr>::Err;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        <Self as FromStr>::from_str(&input)
    }
}
