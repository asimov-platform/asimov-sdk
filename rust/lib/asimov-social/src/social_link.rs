// This is free and unencumbered software released into the public domain.

//! Parsing and formatting recognized social-platform URLs.
//!
//! [`SocialLink`] classifies profile, relationship, list, and content links by
//! their URL structure, without network access. Parsing errors are reported as
//! [`SocialLinkError`]; fallible conversions to account handles or follow
//! relationships use [`SocialLinkConversionError`].
//!
//! Link recognition is independent of the platform feature flags. Conversions
//! involving [`SocialHandle`] require the corresponding platform feature.

use crate::{FollowRelationship, SocialHandle};
use alloc::string::{String, ToString};
use derive_more::Display;
use known_types::handle::ParseHandleError;
use thiserror::Error;
use url::{ParseError, Url};

/// A URL could not be parsed or does not match a supported social link.
///
/// URL syntax is checked first, followed by the scheme, credentials, and port.
/// Host-specific parsing then checks the path, query, and fragment. Values in
/// these errors come from the parsed URL and may have been normalized by [`Url`].
#[derive(Clone, Error, Debug, Eq, PartialEq)]
pub enum SocialLinkError {
    /// The URL uses a scheme other than HTTPS. Contains the scheme without `:`.
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),

    /// The hostname is unsupported, after removing one leading `www.`.
    #[error("unknown URL hostname: {0}")]
    UnknownHost(String),

    /// The URL contains a nonempty username or a password.
    #[error("spurious URL credentials")]
    SpuriousCredentials,

    /// The URL specifies a non-default port. Explicit HTTPS port 443 is accepted.
    #[error("spurious URL port: {0}")]
    SpuriousPort(u16),

    /// An unsupported query, excluding the leading `?` (possibly empty).
    #[error("spurious URL query: {0}")]
    SpuriousQuery(String),

    /// An unsupported fragment, excluding the leading `#` (possibly empty).
    #[error("spurious URL fragment: {0}")]
    SpuriousFragment(String),

    /// An unsupported path, excluding its first `/`.
    #[error("unknown URL path: /{0}")]
    UnknownPath(String),

    /// The underlying URL parser rejected the input.
    #[error("failed to parse URL: {0}")]
    FailedParse(#[from] url::ParseError),
}

/// A social link or handle cannot be represented by the requested type.
#[derive(Clone, Debug, Error)]
pub enum SocialLinkConversionError {
    /// The link is not a plain profile on an enabled, shared platform.
    #[error("link has no supported account-handle representation")]
    UnsupportedLink,

    /// The handle's platform has no corresponding profile-link variant.
    #[error("handle has no supported social-link representation")]
    UnsupportedHandle,

    /// A profile's stored string failed the platform-specific handle parser.
    #[error("invalid account handle: {0}")]
    InvalidHandle(#[from] ParseHandleError),

    /// The link does not select followers, following, or mutuals.
    #[error("link does not represent a follow relationship")]
    NotRelationship,
}

/// A recognized social-platform URL, classified by the resource it selects.
///
/// # Parsing and formatting
///
/// [`core::str::FromStr`] accepts absolute HTTPS URLs on the hosts shown below,
/// with an optional `www.` prefix. URL parsing normalizes host casing, default
/// ports, and dot segments according to [`Url`]. Non-default ports, credentials,
/// and unrecognized queries or fragments are rejected. Paths are case-sensitive;
/// trailing slashes must match the documented forms. An empty `?` or `#` is not
/// treated as absent.
///
/// String payloads contain nonempty URL path segments (or a Luma event query
/// value), retaining percent encoding. Parsing checks URL structure, not platform
/// username rules, IMDb ID prefixes, or whether a resource exists. X list IDs
/// must be positive decimal integers fitting in `i64`.
///
/// [`Display`] and conversion into [`String`] produce the canonical URL shown
/// on each variant, omitting `www.` and normalizing aliases. Public variants can
/// also be constructed directly; their payloads are formatted verbatim without
/// validation or escaping. Equality compares variants and payloads, not URLs.
/// In particular, ambiguous Luma slugs can format identically while representing
/// different variants; see [`LumaPage`](Self::LumaPage).
///
/// # Conversions
///
/// Owned and borrowed `TryFrom` conversions between this type and [`SocialHandle`]
/// support only plain GitHub, Gravatar, Instagram, Intro.co, LinkedIn, Luma, and
/// X profiles, with the respective platform feature enabled. Converting a link
/// validates its string with the platform's handle parser. Other resource types
/// and disabled platforms return [`SocialLinkConversionError::UnsupportedLink`];
/// handles on other platforms return [`SocialLinkConversionError::UnsupportedHandle`].
///
/// Relationship links support fallible conversion to [`FollowRelationship`],
/// also exposed as [`follow_relationship`](Self::follow_relationship).
/// All link variants are available regardless of platform features, except the
/// experimental mutuals and named X list variants, which require `unstable`.
///
/// # Examples
///
/// ```
/// use asimov_social::{FollowRelationship, SocialLink};
///
/// let link: SocialLink = "https://www.x.com/alice/followers".parse()?;
/// assert_eq!(link.to_string(), "https://x.com/alice/followers");
/// assert_eq!(link.follow_relationship(), Some(FollowRelationship::Follower));
/// assert!(SocialLink::XProfile("alice".into()).follow_relationship().is_none());
/// # Ok::<(), asimov_social::SocialLinkError>(())
/// ```
///
/// Converting a profile to a validated account handle:
///
/// ```
/// # #[cfg(feature = "github")]
/// # {
/// use asimov_social::{SocialHandle, SocialLink};
///
/// let link: SocialLink = "https://www.github.com/octocat".parse()?;
/// let handle = SocialHandle::try_from(&link)?;
/// assert_eq!(handle, SocialHandle::github("octocat")?);
/// assert_eq!(SocialLink::try_from(handle)?, link);
///
/// let followers: SocialLink = "https://github.com/octocat?tab=followers".parse()?;
/// assert!(SocialHandle::try_from(followers).is_err());
/// # }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
pub enum SocialLink {
    /// A GitHub profile, storing its handle: `https://github.com/:handle`.
    #[display("https://github.com/{_0}")]
    GithubProfile(String),

    /// Followers of the stored GitHub handle: `https://github.com/:handle?tab=followers`.
    #[display("https://github.com/{_0}?tab=followers")]
    GithubProfileFollowers(String),

    /// Accounts followed by the stored GitHub handle: `https://github.com/:handle?tab=following`.
    #[display("https://github.com/{_0}?tab=following")]
    GithubProfileFollowing(String),

    /// Mutual follows of the stored GitHub handle: `https://github.com/:handle?tab=mutuals`.
    ///
    /// An experimental selector available with `unstable`.
    #[cfg(feature = "unstable")]
    #[display("https://github.com/{_0}?tab=mutuals")]
    GithubProfileMutuals(String),

    /// A Gravatar profile, storing its handle: `https://gravatar.com/:handle`.
    #[display("https://gravatar.com/{_0}")]
    GravatarProfile(String),

    /// An IMDb person, storing the name ID: `https://imdb.com/name/:id/`.
    /// The trailing slash is required; the ID's `nm` prefix is not validated.
    #[display("https://imdb.com/name/{_0}/")]
    ImdbName(String),

    /// An IMDb title, storing the title ID: `https://imdb.com/title/:id/`.
    /// The trailing slash is required; the ID's `tt` prefix is not validated.
    #[display("https://imdb.com/title/{_0}/")]
    ImdbTitle(String),

    /// An Instagram profile, storing its handle: `https://instagram.com/:handle`.
    #[display("https://instagram.com/{_0}")]
    InstagramProfile(String),

    /// Followers of the stored Instagram handle: `https://instagram.com/:handle#followers`.
    #[display("https://instagram.com/{_0}#followers")]
    InstagramProfileFollowers(String),

    /// Accounts followed by the stored Instagram handle: `https://instagram.com/:handle#following`.
    #[display("https://instagram.com/{_0}#following")]
    InstagramProfileFollowing(String),

    /// Mutual follows of the stored Instagram handle: `https://instagram.com/:handle#mutuals`.
    ///
    /// An experimental selector available with `unstable`.
    #[cfg(feature = "unstable")]
    #[display("https://instagram.com/{_0}#mutuals")]
    InstagramProfileMutuals(String),

    /// An Intro.co profile, storing its handle: `https://intro.co/:handle`.
    #[display("https://intro.co/{_0}")]
    IntrocoProfile(String),

    /// A LinkedIn personal profile: `https://linkedin.com/in/:handle/`.
    /// Stores the handle; the trailing slash is required.
    #[display("https://linkedin.com/in/{_0}/")]
    LinkedinProfile(String),

    /// A LinkedIn company page: `https://linkedin.com/company/:handle/`.
    /// Stores the company handle; this is not a personal account handle.
    #[display("https://linkedin.com/company/{_0}/")]
    LinkedinCompanyPage(String),

    /// A Luma calendar, storing its slug: `https://luma.com/:slug`.
    /// Parsing a bare slug selects this variant only for a `cal-` prefix.
    #[display("https://luma.com/{_0}")]
    LumaCalendar(String),

    /// Future events for the stored Luma calendar slug: `https://luma.com/:calendar?period=future`.
    #[display("https://luma.com/{_0}?period=future")]
    LumaCalendarEvents(String),

    /// Past events for the stored Luma calendar slug: `https://luma.com/:calendar?period=past`.
    #[display("https://luma.com/{_0}?period=past")]
    LumaCalendarPastEvents(String),

    /// A Luma event, storing its slug: `https://luma.com/:slug`.
    ///
    /// Recognized from an `evt-` slug or `https://luma.com/:calendar?e=:slug`.
    /// The latter discards the calendar context when formatted. If the event slug
    /// lacks a recognized ID prefix, reparsing that URL yields [`LumaPage`](Self::LumaPage).
    #[display("https://luma.com/{_0}")]
    LumaEvent(String),

    /// An unresolved Luma event or calendar slug: `https://luma.com/:slug`.
    ///
    /// Selected for bare slugs without `cal-`, `evt-`, or `usr-` prefixes.
    /// Distinguishing an event from a calendar requires external information;
    /// this crate does not resolve the slug or perform network requests.
    #[display("https://luma.com/{_0}")]
    LumaPage(String),

    /// A Luma profile, storing its handle: `https://luma.com/user/:handle`.
    /// Also accepts `https://luma.com/usr-...`, retaining the entire `usr-` ID.
    #[display("https://luma.com/user/{_0}")]
    LumaProfile(String),

    /// An X list, storing a positive numeric ID: `https://x.com/i/lists/:id`.
    /// Parsing rejects zero, signs, non-digits, and values exceeding `i64::MAX`.
    #[display("https://x.com/i/lists/{_0}")]
    XList(i64),

    /// An X profile, storing its handle: `https://x.com/:handle`.
    #[display("https://x.com/{_0}")]
    XProfile(String),

    /// Followers of the stored X handle: `https://x.com/:handle/followers`.
    #[display("https://x.com/{_0}/followers")]
    XProfileFollowers(String),

    /// Accounts followed by the stored X handle: `https://x.com/:handle/following`.
    #[display("https://x.com/{_0}/following")]
    XProfileFollowing(String),

    /// Mutual follows of the stored X handle: `https://x.com/:handle/mutuals`.
    ///
    /// An experimental selector available with `unstable`.
    #[cfg(feature = "unstable")]
    #[display("https://x.com/{_0}/mutuals")]
    XProfileMutuals(String),

    /// Highlights of the stored X handle: `https://x.com/:handle/highlights`.
    #[display("https://x.com/{_0}/highlights")]
    XProfileHighlights(String),

    /// Posts by the stored X handle, formatted as `https://x.com/:handle/all`.
    /// Also accepts `https://x.com/:handle/posts` and `https://x.com/:handle#posts`.
    #[display("https://x.com/{_0}/all")]
    XProfilePosts(String),

    /// An X list addressed by owner handle and list slug, in that order:
    /// `https://x.com/:handle/lists/:list`.
    ///
    /// An experimental selector available with `unstable`.
    #[cfg(feature = "unstable")]
    #[display("https://x.com/{_0}/lists/{_1}")]
    XProfileList(String, String),
}

/// Formats the link as its canonical URL without validating its payload.
impl From<SocialLink> for String {
    fn from(input: SocialLink) -> Self {
        input.to_string()
    }
}

impl SocialLink {
    /// Returns the relationship selected by a followers, following, or mutuals link.
    ///
    /// The direction is relative to the account named in the link. Other links
    /// return `None`; a plain profile does not imply any relationship.
    #[must_use]
    pub const fn follow_relationship(&self) -> Option<FollowRelationship> {
        use FollowRelationship::*;
        use SocialLink::*;
        match self {
            GithubProfileFollowers(_) | InstagramProfileFollowers(_) | XProfileFollowers(_) => {
                Some(Follower)
            },

            GithubProfileFollowing(_) | InstagramProfileFollowing(_) | XProfileFollowing(_) => {
                Some(Followee)
            },

            #[cfg(feature = "unstable")]
            GithubProfileMutuals(_) | InstagramProfileMutuals(_) | XProfileMutuals(_) => {
                Some(Mutual)
            },

            _ => None,
        }
    }
}

/// Extracts a relationship, failing for links that do not select one.
impl TryFrom<&SocialLink> for FollowRelationship {
    type Error = SocialLinkConversionError;

    fn try_from(input: &SocialLink) -> Result<Self, Self::Error> {
        input
            .follow_relationship()
            .ok_or(SocialLinkConversionError::NotRelationship)
    }
}

/// The owned equivalent of converting a borrowed link to a relationship.
impl TryFrom<SocialLink> for FollowRelationship {
    type Error = SocialLinkConversionError;

    fn try_from(input: SocialLink) -> Result<Self, Self::Error> {
        Self::try_from(&input)
    }
}

/// Converts a supported account handle into its plain profile link.
impl TryFrom<&SocialHandle> for SocialLink {
    type Error = SocialLinkConversionError;

    fn try_from(input: &SocialHandle) -> Result<Self, Self::Error> {
        match input {
            #[cfg(feature = "github")]
            SocialHandle::Github(h) => Ok(Self::GithubProfile(h.as_str().to_string())),
            #[cfg(feature = "gravatar")]
            SocialHandle::Gravatar(h) => Ok(Self::GravatarProfile(h.as_str().to_string())),
            #[cfg(feature = "instagram")]
            SocialHandle::Instagram(h) => Ok(Self::InstagramProfile(h.as_str().to_string())),
            #[cfg(feature = "introco")]
            SocialHandle::Introco(h) => Ok(Self::IntrocoProfile(h.as_str().to_string())),
            #[cfg(feature = "linkedin")]
            SocialHandle::Linkedin(h) => Ok(Self::LinkedinProfile(h.as_str().to_string())),
            #[cfg(feature = "luma")]
            SocialHandle::Luma(h) => Ok(Self::LumaProfile(h.as_str().to_string())),
            #[cfg(feature = "x")]
            SocialHandle::X(h) => Ok(Self::XProfile(h.as_str().to_string())),
            #[allow(unreachable_patterns)]
            _ => Err(SocialLinkConversionError::UnsupportedHandle),
        }
    }
}

/// The owned equivalent of converting a borrowed handle to a profile link.
impl TryFrom<SocialHandle> for SocialLink {
    type Error = SocialLinkConversionError;

    fn try_from(input: SocialHandle) -> Result<Self, Self::Error> {
        Self::try_from(&input)
    }
}

/// Validates a plain profile link as a handle on an enabled platform.
///
/// Relationship and content links are rejected rather than discarding their
/// resource selection. LinkedIn company handles and Luma calendar/event slugs
/// are likewise not interpreted as personal account handles.
impl TryFrom<&SocialLink> for SocialHandle {
    type Error = SocialLinkConversionError;

    fn try_from(input: &SocialLink) -> Result<Self, Self::Error> {
        match input {
            #[cfg(feature = "github")]
            SocialLink::GithubProfile(h) => Self::github(h).map_err(Into::into),
            #[cfg(feature = "gravatar")]
            SocialLink::GravatarProfile(h) => Self::gravatar(h).map_err(Into::into),
            #[cfg(feature = "instagram")]
            SocialLink::InstagramProfile(h) => Self::instagram(h).map_err(Into::into),
            #[cfg(feature = "introco")]
            SocialLink::IntrocoProfile(h) => Self::introco(h).map_err(Into::into),
            #[cfg(feature = "linkedin")]
            SocialLink::LinkedinProfile(h) => Self::linkedin(h).map_err(Into::into),
            #[cfg(feature = "luma")]
            SocialLink::LumaProfile(h) => Self::luma(h).map_err(Into::into),
            #[cfg(feature = "x")]
            SocialLink::XProfile(h) => Self::x(h).map_err(Into::into),
            _ => Err(SocialLinkConversionError::UnsupportedLink),
        }
    }
}

/// The owned equivalent of converting a borrowed profile link to a handle.
impl TryFrom<SocialLink> for SocialHandle {
    type Error = SocialLinkConversionError;

    fn try_from(input: SocialLink) -> Result<Self, Self::Error> {
        Self::try_from(&input)
    }
}

/// Parses an owned URL string using the same rules as `str::parse`.
impl TryFrom<String> for SocialLink {
    type Error = SocialLinkError;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        input.parse()
    }
}

impl core::str::FromStr for SocialLink {
    type Err = SocialLinkError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        fn handle(path: &str) -> Option<String> {
            if path.is_empty() || path.contains('/') || path.contains('?') || path.contains('#') {
                None
            } else {
                Some(String::from(path))
            }
        }

        let url = Url::parse(input)?;
        if url.scheme() != "https" {
            return Err(SocialLinkError::UnsupportedScheme(url.scheme().to_string()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(SocialLinkError::SpuriousCredentials);
        }
        if let Some(port) = url.port() {
            return Err(SocialLinkError::SpuriousPort(port));
        }
        let host = url.host_str().ok_or(ParseError::EmptyHost)?;
        let host = host.strip_prefix("www.").unwrap_or(host);
        let path = url.path().strip_prefix('/').unwrap_or_default();
        let query = url.query();
        let fragment = url.fragment();

        match host {
            "github.com" => {
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                match query {
                    #[cfg(feature = "unstable")]
                    Some("tab=mutuals") => handle(path)
                        .map(Self::GithubProfileMutuals)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    Some("tab=followers") => handle(path)
                        .map(Self::GithubProfileFollowers)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    Some("tab=following") => handle(path)
                        .map(Self::GithubProfileFollowing)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    None => handle(path)
                        .map(Self::GithubProfile)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    Some(q) => Err(SocialLinkError::SpuriousQuery(q.to_string())),
                }
            },
            "gravatar.com" => {
                if query.is_some() {
                    return Err(SocialLinkError::SpuriousQuery(query.unwrap().to_string()));
                }
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                handle(path)
                    .map(Self::GravatarProfile)
                    .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()))
            },
            "imdb.com" => {
                if query.is_some() {
                    return Err(SocialLinkError::SpuriousQuery(query.unwrap().to_string()));
                }
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                if let Some(id) = path
                    .strip_prefix("name/")
                    .and_then(|path| path.strip_suffix('/'))
                    .and_then(handle)
                {
                    return Ok(Self::ImdbName(id));
                }
                if let Some(id) = path
                    .strip_prefix("title/")
                    .and_then(|path| path.strip_suffix('/'))
                    .and_then(handle)
                {
                    return Ok(Self::ImdbTitle(id));
                }
                Err(SocialLinkError::UnknownPath(path.to_string()))
            },
            "instagram.com" => {
                if query.is_some() {
                    return Err(SocialLinkError::SpuriousQuery(query.unwrap().to_string()));
                }
                match fragment {
                    #[cfg(feature = "unstable")]
                    Some("mutuals") => handle(path)
                        .map(Self::InstagramProfileMutuals)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    Some("followers") => handle(path)
                        .map(Self::InstagramProfileFollowers)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    Some("following") => handle(path)
                        .map(Self::InstagramProfileFollowing)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    None => handle(path)
                        .map(Self::InstagramProfile)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string())),
                    Some(_) => Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    )),
                }
            },
            "intro.co" => {
                if query.is_some() {
                    return Err(SocialLinkError::SpuriousQuery(query.unwrap().to_string()));
                }
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                handle(path)
                    .map(Self::IntrocoProfile)
                    .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()))
            },
            "linkedin.com" => {
                if query.is_some() {
                    return Err(SocialLinkError::SpuriousQuery(query.unwrap().to_string()));
                }
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                if let Some(handle) = path
                    .strip_prefix("in/")
                    .and_then(|path| path.strip_suffix('/'))
                    .and_then(handle)
                {
                    return Ok(Self::LinkedinProfile(handle));
                }
                if let Some(handle) = path
                    .strip_prefix("company/")
                    .and_then(|path| path.strip_suffix('/'))
                    .and_then(handle)
                {
                    return Ok(Self::LinkedinCompanyPage(handle));
                }
                Err(SocialLinkError::UnknownPath(path.to_string()))
            },
            "luma.com" => {
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                if let Some(value) = query.and_then(|q| q.strip_prefix("e=")) {
                    if handle(path).is_none() {
                        return Err(SocialLinkError::UnknownPath(path.to_string()));
                    }
                    let event = handle(value)
                        .filter(|_| !value.contains('&') && !value.contains('='))
                        .ok_or_else(|| {
                            SocialLinkError::SpuriousQuery(query.unwrap().to_string())
                        })?;
                    return Ok(Self::LumaEvent(event));
                }
                match query {
                    None => {},
                    Some("period=future") => {
                        return handle(path)
                            .map(Self::LumaCalendarEvents)
                            .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()));
                    },
                    Some("period=past") => {
                        return handle(path)
                            .map(Self::LumaCalendarPastEvents)
                            .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()));
                    },
                    Some(q) => return Err(SocialLinkError::SpuriousQuery(q.to_string())),
                }
                if let Some(handle) = path.strip_prefix("user/").and_then(handle) {
                    return Ok(Self::LumaProfile(handle));
                }
                match handle(path) {
                    Some(slug) if slug.starts_with("cal-") => Ok(Self::LumaCalendar(slug)),
                    Some(slug) if slug.starts_with("evt-") => Ok(Self::LumaEvent(slug)),
                    Some(slug) if slug.starts_with("usr-") => Ok(Self::LumaProfile(slug)),
                    Some(slug) => Ok(Self::LumaPage(slug)),
                    None => Err(SocialLinkError::UnknownPath(path.to_string())),
                }
            },
            "x.com" => {
                if query.is_some() {
                    return Err(SocialLinkError::SpuriousQuery(query.unwrap().to_string()));
                }
                if fragment == Some("posts") {
                    return handle(path)
                        .map(Self::XProfilePosts)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()));
                }
                if fragment.is_some() {
                    return Err(SocialLinkError::SpuriousFragment(
                        fragment.unwrap().to_string(),
                    ));
                }
                if let Some(id) = path.strip_prefix("i/lists/") {
                    return id
                        .parse::<i64>()
                        .ok()
                        .filter(|id| *id > 0)
                        .filter(|_| id.bytes().all(|b| b.is_ascii_digit()))
                        .map(Self::XList)
                        .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()));
                }
                if let Some(handle) = path.strip_suffix("/followers").and_then(handle) {
                    return Ok(Self::XProfileFollowers(handle));
                }
                if let Some(handle) = path.strip_suffix("/following").and_then(handle) {
                    return Ok(Self::XProfileFollowing(handle));
                }
                #[cfg(feature = "unstable")]
                if let Some(handle) = path.strip_suffix("/mutuals").and_then(handle) {
                    return Ok(Self::XProfileMutuals(handle));
                }
                #[cfg(feature = "unstable")]
                if let Some((owner, list)) = path.split_once("/lists/") {
                    if let (Some(owner), Some(list)) = (handle(owner), handle(list)) {
                        return Ok(Self::XProfileList(owner, list));
                    }
                    return Err(SocialLinkError::UnknownPath(path.to_string()));
                }
                if let Some(handle) = path.strip_suffix("/highlights").and_then(handle) {
                    return Ok(Self::XProfileHighlights(handle));
                }
                if let Some(handle) = path.strip_suffix("/all").and_then(handle) {
                    return Ok(Self::XProfilePosts(handle));
                }
                if let Some(handle) = path.strip_suffix("/posts").and_then(handle) {
                    return Ok(Self::XProfilePosts(handle));
                }
                handle(path)
                    .map(Self::XProfile)
                    .ok_or_else(|| SocialLinkError::UnknownPath(path.to_string()))
            },
            _ => Err(SocialLinkError::UnknownHost(host.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SocialLink, SocialLinkConversionError, SocialLinkError};
    use crate::{FollowRelationship, SocialHandle};
    use alloc::{
        format,
        string::{String, ToString},
    };

    #[test]
    fn canonical_urls_round_trip() {
        for input in [
            "https://github.com/alice",
            "https://github.com/alice?tab=followers",
            "https://github.com/alice?tab=following",
            "https://gravatar.com/alice",
            "https://imdb.com/name/nm123/",
            "https://imdb.com/title/tt123/",
            "https://instagram.com/alice",
            "https://instagram.com/alice#followers",
            "https://instagram.com/alice#following",
            "https://intro.co/alice",
            "https://linkedin.com/in/alice/",
            "https://linkedin.com/company/acme/",
            "https://luma.com/cal-abc",
            "https://luma.com/claw?period=future",
            "https://luma.com/claw?period=past",
            "https://luma.com/evt-abc",
            "https://luma.com/claw",
            "https://luma.com/user/alice",
            "https://x.com/i/lists/9223372036854775807",
            "https://x.com/alice",
            "https://x.com/alice/followers",
            "https://x.com/alice/following",
            "https://x.com/alice/highlights",
            "https://x.com/alice/all",
        ] {
            let link: SocialLink = input.parse().unwrap();
            assert_eq!(link.to_string(), input);
            assert_eq!(
                SocialLink::try_from(String::from(link.clone())).unwrap(),
                link
            );
        }
    }

    #[test]
    fn aliases_and_url_normalization() {
        for (input, canonical) in [
            (
                "https://WWW.GITHUB.COM:443/alice",
                "https://github.com/alice",
            ),
            ("https://x.com/alice/posts", "https://x.com/alice/all"),
            ("https://x.com/alice#posts", "https://x.com/alice/all"),
            ("https://x.com/i/lists/001", "https://x.com/i/lists/1"),
            ("https://luma.com/usr-abc", "https://luma.com/user/usr-abc"),
        ] {
            let link: SocialLink = input.parse().unwrap();
            assert_eq!(link.to_string(), canonical);
            assert_eq!(canonical.parse::<SocialLink>().unwrap(), link);
        }

        let event: SocialLink = "https://luma.com/claw?e=custom-event".parse().unwrap();
        assert_eq!(event, SocialLink::LumaEvent("custom-event".into()));
        assert_eq!(event.to_string(), "https://luma.com/custom-event");
        assert_eq!(
            event.to_string().parse::<SocialLink>().unwrap(),
            SocialLink::LumaPage("custom-event".into())
        );
    }

    #[test]
    fn rejects_malformed_luma_event_queries() {
        for query in [
            "e=",
            "e=event&extra=1",
            "e=event&e=other",
            "e=event=other",
            "e=a/b",
        ] {
            assert!(matches!(
                format!("https://luma.com/calendar?{query}").parse::<SocialLink>(),
                Err(SocialLinkError::SpuriousQuery(_))
            ));
        }
        for path in ["", "calendar/extra", "user/alice"] {
            assert!(matches!(
                format!("https://luma.com/{path}?e=event").parse::<SocialLink>(),
                Err(SocialLinkError::UnknownPath(_))
            ));
        }
    }

    #[test]
    fn rejects_credentials_ports_and_empty_selectors() {
        assert_eq!(
            "https://github.com:8443/alice".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousPort(8443))
        );
        assert_eq!(
            "https://:secret@github.com/alice".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousCredentials)
        );
        assert_eq!(
            "https://github.com/alice?".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousQuery("".into()))
        );
        assert_eq!(
            "https://github.com/alice#".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousFragment("".into()))
        );
    }

    #[test]
    fn relationship_conversion_is_fallible() {
        for link in [
            SocialLink::GithubProfileFollowers("alice".into()),
            SocialLink::InstagramProfileFollowers("alice".into()),
            SocialLink::XProfileFollowers("alice".into()),
        ] {
            assert_eq!(
                link.follow_relationship(),
                Some(FollowRelationship::Follower)
            );
            assert_eq!(
                FollowRelationship::try_from(&link).unwrap(),
                FollowRelationship::Follower
            );
            assert_eq!(
                FollowRelationship::try_from(link).unwrap(),
                FollowRelationship::Follower
            );
        }
        for link in [
            SocialLink::GithubProfileFollowing("alice".into()),
            SocialLink::InstagramProfileFollowing("alice".into()),
            SocialLink::XProfileFollowing("alice".into()),
        ] {
            assert_eq!(
                FollowRelationship::try_from(&link).unwrap(),
                FollowRelationship::Followee
            );
        }
        for link in [
            SocialLink::XProfile("alice".into()),
            SocialLink::XList(1),
            SocialLink::XProfilePosts("alice".into()),
        ] {
            assert_eq!(link.follow_relationship(), None);
            assert!(matches!(
                FollowRelationship::try_from(&link),
                Err(SocialLinkConversionError::NotRelationship)
            ));
        }
    }

    macro_rules! profile_conversion_test {
        ($feature:literal, $constructor:ident, $variant:ident) => {
            #[cfg(feature = $feature)]
            #[test]
            fn $constructor() {
                let handle = SocialHandle::$constructor("alice").unwrap();
                let expected = SocialLink::$variant(handle.as_str().into());
                assert_eq!(SocialLink::try_from(&handle).unwrap(), expected);
                assert_eq!(SocialLink::try_from(handle.clone()).unwrap(), expected);
                assert_eq!(SocialHandle::try_from(&expected).unwrap(), handle);
                assert_eq!(SocialHandle::try_from(expected.clone()).unwrap(), handle);
                assert_eq!(
                    SocialHandle::try_from(expected.to_string().parse::<SocialLink>().unwrap())
                        .unwrap(),
                    handle
                );
                assert!(matches!(
                    SocialHandle::try_from(SocialLink::$variant("".into())),
                    Err(SocialLinkConversionError::InvalidHandle(_))
                ));
            }
        };
    }

    profile_conversion_test!("github", github, GithubProfile);
    profile_conversion_test!("gravatar", gravatar, GravatarProfile);
    profile_conversion_test!("instagram", instagram, InstagramProfile);
    profile_conversion_test!("introco", introco, IntrocoProfile);
    profile_conversion_test!("linkedin", linkedin, LinkedinProfile);
    profile_conversion_test!("luma", luma, LumaProfile);
    profile_conversion_test!("x", x, XProfile);

    #[cfg(feature = "github")]
    #[test]
    fn parsed_profile_still_requires_handle_validation() {
        let link: SocialLink = "https://github.com/not%20a%20handle".parse().unwrap();
        assert_eq!(link, SocialLink::GithubProfile("not%20a%20handle".into()));
        assert!(matches!(
            SocialHandle::try_from(link),
            Err(SocialLinkConversionError::InvalidHandle(_))
        ));
    }

    #[test]
    fn non_profile_links_do_not_convert_to_handles() {
        for link in [
            SocialLink::GithubProfileFollowers("alice".into()),
            SocialLink::GithubProfileFollowing("alice".into()),
            SocialLink::InstagramProfileFollowers("alice".into()),
            SocialLink::InstagramProfileFollowing("alice".into()),
            SocialLink::XProfileFollowers("alice".into()),
            SocialLink::XProfileFollowing("alice".into()),
            SocialLink::XProfileHighlights("alice".into()),
            SocialLink::XProfilePosts("alice".into()),
            SocialLink::XList(1),
            SocialLink::LinkedinCompanyPage("acme".into()),
            SocialLink::ImdbName("nm123".into()),
            SocialLink::ImdbTitle("tt123".into()),
            SocialLink::LumaCalendar("cal-abc".into()),
            SocialLink::LumaCalendarEvents("calendar".into()),
            SocialLink::LumaCalendarPastEvents("calendar".into()),
            SocialLink::LumaEvent("evt-abc".into()),
            SocialLink::LumaPage("alice".into()),
        ] {
            assert!(matches!(
                SocialHandle::try_from(&link),
                Err(SocialLinkConversionError::UnsupportedLink)
            ));
            assert!(matches!(
                SocialHandle::try_from(link),
                Err(SocialLinkConversionError::UnsupportedLink)
            ));
        }
    }

    #[cfg(feature = "facebook")]
    #[test]
    fn unsupported_handle_platform() {
        let handle = SocialHandle::facebook("alice").unwrap();
        assert!(matches!(
            SocialLink::try_from(&handle),
            Err(SocialLinkConversionError::UnsupportedHandle)
        ));
        assert!(matches!(
            SocialLink::try_from(handle),
            Err(SocialLinkConversionError::UnsupportedHandle)
        ));
    }

    #[cfg(not(feature = "github"))]
    #[test]
    fn disabled_platform_still_parses_links() {
        let link: SocialLink = "https://github.com/alice".parse().unwrap();
        assert_eq!(link, SocialLink::GithubProfile("alice".into()));
        assert!(matches!(
            SocialHandle::try_from(link),
            Err(SocialLinkConversionError::UnsupportedLink)
        ));
    }

    #[cfg(feature = "unstable")]
    #[test]
    fn experimental_links_round_trip() {
        for (input, expected) in [
            (
                "https://github.com/alice?tab=mutuals",
                SocialLink::GithubProfileMutuals("alice".into()),
            ),
            (
                "https://instagram.com/alice#mutuals",
                SocialLink::InstagramProfileMutuals("alice".into()),
            ),
            (
                "https://x.com/alice/mutuals",
                SocialLink::XProfileMutuals("alice".into()),
            ),
        ] {
            let link: SocialLink = input.parse().unwrap();
            assert_eq!(link, expected);
            assert_eq!(link.to_string(), input);
            assert_eq!(link.follow_relationship(), Some(FollowRelationship::Mutual));
            assert!(matches!(
                SocialHandle::try_from(link),
                Err(SocialLinkConversionError::UnsupportedLink)
            ));
        }
        let input = "https://x.com/alice/lists/friends";
        let link: SocialLink = input.parse().unwrap();
        assert_eq!(
            link,
            SocialLink::XProfileList("alice".into(), "friends".into())
        );
        assert_eq!(link.to_string(), input);
        for input in [
            "https://x.com/alice/lists/",
            "https://x.com/alice/lists/a/b",
        ] {
            assert!(input.parse::<SocialLink>().is_err());
        }
    }

    #[cfg(not(feature = "unstable"))]
    #[test]
    fn experimental_links_require_feature() {
        for input in [
            "https://github.com/alice?tab=mutuals",
            "https://instagram.com/alice#mutuals",
            "https://x.com/alice/mutuals",
            "https://x.com/alice/lists/friends",
        ] {
            assert!(input.parse::<SocialLink>().is_err());
        }
    }

    #[test]
    fn x_lists() {
        let url = "https://x.com/i/lists/1234567890123456789";
        let parsed = url.parse::<SocialLink>().unwrap();
        assert!(matches!(parsed, SocialLink::XList(1234567890123456789)));
        assert_eq!(parsed.to_string(), url);
        for id in [
            "",
            "abc",
            "0",
            "-1",
            "+1",
            "9223372036854775808",
            "123/members",
        ] {
            assert!(
                format!("https://x.com/i/lists/{id}")
                    .parse::<SocialLink>()
                    .is_err()
            );
        }
    }

    macro_rules! assert_parses_as {
        ($input:literal, $variant:ident, $expected:literal) => {
            match $input.parse::<SocialLink>() {
                Ok(SocialLink::$variant(handle)) => {
                    assert_eq!(handle, $expected, "unexpected handle for {}", $input)
                },
                Ok(_) => panic!("unexpected variant for {}", $input),
                Err(_) => panic!("failed to parse {}", $input),
            }
        };
    }

    #[test]
    fn parses_documented_links() {
        assert_parses_as!("https://github.com/alice", GithubProfile, "alice");
        assert_parses_as!(
            "https://github.com/alice?tab=followers",
            GithubProfileFollowers,
            "alice"
        );
        assert_parses_as!(
            "https://github.com/alice?tab=following",
            GithubProfileFollowing,
            "alice"
        );

        assert_parses_as!("https://gravatar.com/alice", GravatarProfile, "alice");

        assert_parses_as!("https://imdb.com/name/nm1234567/", ImdbName, "nm1234567");
        assert_parses_as!("https://imdb.com/title/tt1234567/", ImdbTitle, "tt1234567");

        assert_parses_as!("https://instagram.com/alice", InstagramProfile, "alice");
        assert_parses_as!(
            "https://instagram.com/alice#followers",
            InstagramProfileFollowers,
            "alice"
        );
        assert_parses_as!(
            "https://instagram.com/alice#following",
            InstagramProfileFollowing,
            "alice"
        );

        assert_parses_as!("https://intro.co/alice", IntrocoProfile, "alice");

        assert_parses_as!("https://linkedin.com/in/alice/", LinkedinProfile, "alice");
        assert_parses_as!(
            "https://linkedin.com/company/acme/",
            LinkedinCompanyPage,
            "acme"
        );
        assert_parses_as!(
            "https://www.linkedin.com/in/alice/",
            LinkedinProfile,
            "alice"
        );

        assert_parses_as!("https://luma.com/user/alice", LumaProfile, "alice");
        assert_parses_as!("https://luma.com/usr-abc", LumaProfile, "usr-abc");
        assert_parses_as!("https://luma.com/cal-abc", LumaCalendar, "cal-abc");
        assert_parses_as!("https://luma.com/evt-abc", LumaEvent, "evt-abc");
        assert_parses_as!("https://luma.com/claw?e=y1eszitt", LumaEvent, "y1eszitt");
        assert_parses_as!("https://luma.com/y1eszitt", LumaPage, "y1eszitt");
        assert_parses_as!(
            "https://luma.com/claw?period=future",
            LumaCalendarEvents,
            "claw"
        );
        assert_parses_as!(
            "https://luma.com/cal-abc?period=past",
            LumaCalendarPastEvents,
            "cal-abc"
        );

        assert_parses_as!("https://x.com/alice", XProfile, "alice");
        assert_parses_as!("https://x.com/alice/followers", XProfileFollowers, "alice");
        assert_parses_as!("https://x.com/alice/following", XProfileFollowing, "alice");
        assert_parses_as!(
            "https://x.com/alice/highlights",
            XProfileHighlights,
            "alice"
        );
        assert_parses_as!("https://x.com/alice/all", XProfilePosts, "alice");
        assert_parses_as!("https://x.com/alice/posts", XProfilePosts, "alice");
        assert_parses_as!("https://x.com/alice#posts", XProfilePosts, "alice");
    }

    #[test]
    fn rejects_unsupported_or_malformed_links() {
        for input in [
            "",
            "http://github.com/alice",
            "https://github.com/",
            "https://github.com/alice/extra",
            "https://github.com/alice?tab=unknown",
            "https://gravatar.com/",
            "https://imdb.com/name/nm1234567",
            "https://imdb.com/name/nm1234567//",
            "https://instagram.com/alice#unknown",
            "https://intro.co/alice/extra",
            "https://linkedin.com/in/alice",
            "https://linkedin.com/company/acme",
            "https://luma.com/",
            "https://luma.com/user/",
            "https://luma.com/alice/extra",
            "https://luma.com/alice?k=c",
            "https://luma.com/alice?period=now",
            "https://luma.com/?period=past",
            "https://luma.com/alice#guests",
            "https://x.com/",
            "https://x.com/alice/unknown",
        ] {
            assert!(input.parse::<SocialLink>().is_err(), "accepted {}", input);
        }
    }

    #[test]
    fn reports_specific_errors() {
        assert!(matches!(
            "".parse::<SocialLink>(),
            Err(SocialLinkError::FailedParse(_))
        ));
        assert!(matches!(
            "http://github.com/alice".parse::<SocialLink>(),
            Err(SocialLinkError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            "https://example.com/alice".parse::<SocialLink>(),
            Err(SocialLinkError::UnknownHost(_))
        ));
        assert!(matches!(
            "https://user@github.com/alice".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousCredentials)
        ));
        assert!(matches!(
            "https://github.com/alice?tab=unknown".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousQuery(_))
        ));
        assert!(matches!(
            "https://github.com/alice#unknown".parse::<SocialLink>(),
            Err(SocialLinkError::SpuriousFragment(_))
        ));
        assert!(matches!(
            "https://github.com/".parse::<SocialLink>(),
            Err(SocialLinkError::UnknownPath(_))
        ));
    }
}
