// This is free and unencumbered software released into the public domain.

use core::str::FromStr;
use derive_more::{Display, From, FromStrError};

#[derive(Clone, Debug, Display, Eq, From, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum SocialPlatform {
    /// Bluesky
    #[display("https://bsky.app")]
    Bluesky,

    /// Facebook
    #[display("https://facebook.com")]
    Facebook,

    /// GitHub
    #[display("https://github.com")]
    Github,

    /// GitLab
    #[display("https://gitlab.com")]
    Gitlab,

    /// Gravatar
    #[display("https://gravatar.com")]
    Gravatar,

    /// Instagram
    #[display("https://instagram.com")]
    Instagram,

    /// Intro.co
    #[display("https://intro.co")]
    Introco,

    /// LinkedIn
    #[display("https://linkedin.com")]
    Linkedin,

    /// local.ai
    #[display("https://local.ai")]
    Localai,

    /// Luma
    #[display("https://luma.com")]
    Luma,

    /// Medium
    #[display("https://medium.com")]
    Medium,

    /// Substack
    #[display("https://substack.com")]
    Substack,

    /// Telegram
    #[display("https://t.me")]
    Telegram,

    /// TikTok
    #[display("https://tiktok.com")]
    Tiktok,

    /// Threads
    #[display("https://threads.net")]
    Threads,

    /// WhatsApp
    #[display("https://wa.me")]
    Whatsapp,

    /// X (fka Twitter)
    #[display("https://x.com")]
    X,

    /// YouTube
    #[display("https://youtube.com")]
    Youtube,
}
