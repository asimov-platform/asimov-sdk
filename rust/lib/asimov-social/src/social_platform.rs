// This is free and unencumbered software released into the public domain.

use core::str::FromStr;
use derive_more::{Display, From, FromStrError};

/// A social platform, identified by its canonical HTTPS base URL.
///
/// [`Display`] formats the platform's HTTPS base URL without a trailing slash;
/// [`FromStr`] accepts those base URLs, optionally with one trailing
/// slash and an optional `www.` hostname prefix, discarded on output.
/// It does not infer a platform from a profile URL, bare domain, or name.
/// Threads' `threads.com` alias is also accepted.
/// Parsing normalizes scheme/host casing, default ports, and dot segments using
/// [`url::Url`]; the resulting path must be `/`, without a query or fragment.
/// Use [`crate::SocialLink`] to recognize supported social resource URLs.
///
/// All variants are available regardless of platform feature flags. This type
/// provides [`handle`](Self::handle) to parse an identifier when the corresponding
/// feature is enabled. Profile URL conventions are documented on [`crate::SocialLink`].
/// The enum is non-exhaustive so more platforms can be added in future releases.
///
/// # Examples
///
/// ```
/// use asimov_social::SocialPlatform;
///
/// let platform: SocialPlatform = "https://bsky.app".parse()?;
/// assert_eq!(platform, SocialPlatform::Bluesky);
/// let profile = format!("{}/{}{}", platform,
///     platform.handle_prefix().unwrap_or(""), "alice.bsky.social");
/// assert_eq!(profile, "https://bsky.app/profile/alice.bsky.social");
/// # Ok::<(), derive_more::FromStrError>(())
/// ```
#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum SocialPlatform {
    /// Bluesky.
    #[display("https://bsky.app")]
    Bluesky,

    /// Discord.
    #[display("https://discord.com")]
    Discord,

    /// Facebook.
    #[display("https://facebook.com")]
    Facebook,

    /// GitHub.
    #[display("https://github.com")]
    Github,

    /// GitLab.com; self-managed instances are not represented.
    #[display("https://gitlab.com")]
    Gitlab,

    /// Gravatar.
    #[display("https://gravatar.com")]
    Gravatar,

    /// Instagram.
    #[display("https://instagram.com")]
    Instagram,

    /// Intro.co.
    #[display("https://intro.co")]
    Introco,

    /// LinkedIn.
    #[display("https://linkedin.com")]
    Linkedin,

    /// local.ai.
    #[display("https://local.ai")]
    Localai,

    /// Luma.
    #[display("https://luma.com")]
    Luma,

    /// Medium.
    #[display("https://medium.com")]
    Medium,

    /// Pinterest.
    #[display("https://pinterest.com")]
    Pinterest,

    /// Reddit.
    #[display("https://reddit.com")]
    Reddit,

    /// Snapchat.
    #[display("https://snapchat.com")]
    Snapchat,

    /// Substack.
    #[display("https://substack.com")]
    Substack,

    /// Telegram, using its `t.me` link domain.
    #[display("https://t.me")]
    Telegram,

    /// Threads, retaining the `threads.net` base for compatibility.
    #[display("https://threads.net")]
    Threads,

    /// TikTok.
    #[display("https://tiktok.com")]
    Tiktok,

    /// Twitch.
    #[display("https://twitch.tv")]
    Twitch,

    /// WhatsApp, using its `wa.me` contact domain.
    #[display("https://wa.me")]
    Whatsapp,

    /// X (formerly Twitter).
    #[display("https://x.com")]
    X,

    /// YouTube.
    #[display("https://youtube.com")]
    Youtube,
}

impl SocialPlatform {
    /// Returns the literal path prefix immediately before an account identifier.
    ///
    /// Follows the base URL and `/`. `None` means the identifier goes directly
    /// after that slash. Prefixes include any separator before the identifier
    /// (`in/`, `@`, etc.), but no leading slash. This metadata does not validate
    /// or escape identifiers; use [`Self::handle`] and [`crate::SocialLink`] for
    /// typed handles and resource-specific URL formatting.
    ///
    /// # Examples
    ///
    /// ```
    /// use asimov_social::SocialPlatform;
    ///
    /// assert_eq!(SocialPlatform::Bluesky.handle_prefix(), Some("profile/"));
    /// assert_eq!(SocialPlatform::Linkedin.handle_prefix(), Some("in/"));
    /// assert_eq!(SocialPlatform::Luma.handle_prefix(), Some("user/"));
    /// assert_eq!(SocialPlatform::X.handle_prefix(), None);
    /// assert_eq!(SocialPlatform::Youtube.handle_prefix(), Some("@"));
    /// ```
    pub const fn handle_prefix(&self) -> Option<&'static str> {
        use SocialPlatform::*;
        Some(match self {
            Bluesky => "profile/",
            Discord => "users/",
            Facebook => return None,
            Github => return None,
            Gitlab => return None,
            Gravatar => return None,
            Instagram => return None,
            Introco => return None,
            Linkedin => "in/",
            Localai => return None,
            Luma => "user/",
            Medium => "@",
            Pinterest => return None,
            Reddit => "user/",
            Snapchat => "add/",
            Substack => "@",
            Telegram => return None,
            Threads => "@",
            Tiktok => "@",
            Twitch => return None,
            Whatsapp => return None,
            X => return None,
            Youtube => "@",
        })
    }

    /// Parses an account identifier on this platform, without a URL prefix.
    ///
    /// Uses the same validation as the corresponding [`crate::SocialHandle`]
    /// constructor. Discord expects a numeric user ID; WhatsApp expects a username,
    /// not a phone number. A disabled platform feature returns
    /// [`SocialPlatformHandleError::DisabledPlatform`].
    ///
    /// ```
    /// # #[cfg(feature = "reddit")]
    /// # {
    /// use asimov_social::{SocialHandle, SocialPlatform};
    /// assert_eq!(SocialPlatform::Reddit.handle("alice")?, SocialHandle::reddit("alice")?);
    /// # }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn handle(
        &self,
        _input: impl AsRef<str>,
    ) -> Result<crate::SocialHandle, SocialPlatformHandleError> {
        macro_rules! dispatch {
            ($($feature:literal => $variant:ident, $constructor:ident;)*) => {
                match self {
                    $(#[cfg(feature = $feature)]
                    Self::$variant => crate::SocialHandle::$constructor(_input)
                        .map_err(SocialPlatformHandleError::InvalidHandle),)*
                    #[allow(unreachable_patterns)]
                    _ => Err(SocialPlatformHandleError::DisabledPlatform(*self)),
                }
            };
        }
        dispatch! {
            "bluesky" => Bluesky, bluesky;
            "discord" => Discord, discord;
            "facebook" => Facebook, facebook;
            "github" => Github, github;
            "gitlab" => Gitlab, gitlab;
            "gravatar" => Gravatar, gravatar;
            "instagram" => Instagram, instagram;
            "introco" => Introco, introco;
            "linkedin" => Linkedin, linkedin;
            "localai" => Localai, localai;
            "luma" => Luma, luma;
            "medium" => Medium, medium;
            "pinterest" => Pinterest, pinterest;
            "reddit" => Reddit, reddit;
            "snapchat" => Snapchat, snapchat;
            "substack" => Substack, substack;
            "telegram" => Telegram, telegram;
            "threads" => Threads, threads;
            "tiktok" => Tiktok, tiktok;
            "twitch" => Twitch, twitch;
            "whatsapp" => Whatsapp, whatsapp;
            "x" => X, x;
            "youtube" => Youtube, youtube;
        }
    }
}

/// An account identifier could not be parsed on the selected platform.
#[derive(Clone, Debug, thiserror::Error)]
pub enum SocialPlatformHandleError {
    /// Enable the platform's Cargo feature to construct its handles.
    #[error("handle support is disabled for {0}")]
    DisabledPlatform(SocialPlatform),
    /// The platform-specific identifier parser rejected the input.
    #[error("invalid account identifier: {0}")]
    InvalidHandle(#[from] known_types::handle::ParseHandleError),
}

impl FromStr for SocialPlatform {
    type Err = FromStrError;

    /// Parses an HTTPS base URL using [`url::Url`] normalization.
    ///
    /// Accepts an optional `www.` hostname prefix and default port 443. Rejects
    /// surrounding whitespace, credentials, non-default ports, non-root paths,
    /// queries, and fragments with [`FromStrError`].
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.trim() != input {
            return Err(FromStrError::new("SocialPlatform"));
        }
        let url =
            crate::social_url::parse(input).map_err(|_| FromStrError::new("SocialPlatform"))?;
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(FromStrError::new("SocialPlatform"));
        }
        Self::from_host(url.host_str().ok_or(FromStrError::new("SocialPlatform"))?)
    }
}

impl SocialPlatform {
    /// Classifies a hostname already normalized by the shared URL parser.
    pub(crate) fn from_host(host: &str) -> Result<Self, FromStrError> {
        match host {
            "bsky.app" => Ok(Self::Bluesky),
            "discord.com" => Ok(Self::Discord),
            "facebook.com" => Ok(Self::Facebook),
            "github.com" => Ok(Self::Github),
            "gitlab.com" => Ok(Self::Gitlab),
            "gravatar.com" => Ok(Self::Gravatar),
            "instagram.com" => Ok(Self::Instagram),
            "intro.co" => Ok(Self::Introco),
            "linkedin.com" => Ok(Self::Linkedin),
            "local.ai" => Ok(Self::Localai),
            "luma.com" => Ok(Self::Luma),
            "medium.com" => Ok(Self::Medium),
            "pinterest.com" => Ok(Self::Pinterest),
            "reddit.com" => Ok(Self::Reddit),
            "snapchat.com" => Ok(Self::Snapchat),
            "substack.com" => Ok(Self::Substack),
            "t.me" => Ok(Self::Telegram),
            "threads.com" | "threads.net" => Ok(Self::Threads),
            "tiktok.com" => Ok(Self::Tiktok),
            "twitch.tv" => Ok(Self::Twitch),
            "wa.me" => Ok(Self::Whatsapp),
            "x.com" => Ok(Self::X),
            "youtube.com" => Ok(Self::Youtube),
            _ => Err(FromStrError::new("SocialPlatform")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SocialPlatform;
    use alloc::{format, string::ToString};

    #[test]
    fn base_urls_round_trip() {
        use SocialPlatform::*;
        for platform in [
            Bluesky, Discord, Facebook, Github, Gitlab, Gravatar, Instagram, Introco, Linkedin,
            Localai, Luma, Medium, Pinterest, Reddit, Snapchat, Substack, Telegram, Threads,
            Tiktok, Twitch, Whatsapp, X, Youtube,
        ] {
            let url = platform.to_string();
            assert!(!url.ends_with('/'));
            assert_eq!(url.parse::<SocialPlatform>(), Ok(platform));
            assert_eq!(format!("{url}/").parse::<SocialPlatform>(), Ok(platform));
            let www = url.replacen("https://", "https://www.", 1);
            assert_eq!(www.parse::<SocialPlatform>(), Ok(platform));
            assert_eq!(format!("{www}/").parse::<SocialPlatform>(), Ok(platform));
            assert_eq!(www.parse::<SocialPlatform>().unwrap().to_string(), url);
            assert!(format!("{url}//").parse::<SocialPlatform>().is_err());
            assert!(format!("{url}/someone").parse::<SocialPlatform>().is_err());
        }
    }

    #[test]
    fn rejects_non_base_urls() {
        for input in [
            "",
            "X",
            "x.com",
            "http://x.com",
            " https://x.com",
            "https://x.com ",
            "https://x.com?query",
            "https://x.com?",
            "https://x.com#fragment",
            "https://x.com#",
            "https://x.com.example.org",
            "https://twitter.com",
            "https://example.org",
            "https://www.www.x.com",
            "https://www.x.com.example.org",
            "https://www.x.com@evil.example",
        ] {
            assert!(input.parse::<SocialPlatform>().is_err(), "{input:?}");
        }
    }

    #[test]
    fn normalizes_base_url_components() {
        for input in [
            "HTTPS://WWW.X.COM:443/",
            "https://www.%78.com",
            "https://www.x.com/unused/..",
        ] {
            let platform: SocialPlatform = input.parse().unwrap();
            assert_eq!(platform, SocialPlatform::X);
            assert_eq!(platform.to_string(), "https://x.com");
        }
    }
}
