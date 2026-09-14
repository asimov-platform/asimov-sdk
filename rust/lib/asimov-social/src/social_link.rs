// This is free and unencumbered software released into the public domain.

use crate::FollowRelationship;
use alloc::string::{String, ToString};
use derive_more::Display;
use thiserror::Error;
use url::{ParseError, Url};

#[derive(Clone, Error, Debug)]
pub enum SocialLinkError {
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),

    #[error("unknown URL hostname: {0}")]
    UnknownHost(String),

    #[error("spurious URL credentials")]
    SpuriousCredentials,

    #[error("spurious URL query: {0}")]
    SpuriousQuery(String),

    #[error("spurious URL fragment: {0}")]
    SpuriousFragment(String),

    #[error("unknown URL path: /{0}")]
    UnknownPath(String),

    #[error("failed to parse URL: {0}")]
    FailedParse(#[from] url::ParseError),
}

/// Well-known links.
#[derive(Clone, Debug, Display, Hash)]
pub enum SocialLink {
    /// `https://github.com/:handle`
    #[display("https://github.com/{_0}")]
    GithubProfile(String),

    /// `https://github.com/:handle?tab=followers`
    #[display("https://github.com/{_0}?tab=followers")]
    GithubProfileFollowers(String),

    /// `https://github.com/:handle?tab=following`
    #[display("https://github.com/{_0}?tab=following")]
    GithubProfileFollowing(String),

    /// `https://github.com/:handle?tab=mutuals`
    #[cfg(feature = "unstable")] // TODO
    #[display("https://github.com/{_0}?tab=mutuals")]
    GithubProfileMutuals(String),

    /// `https://gravatar.com/:handle`
    #[display("https://gravatar.com/{_0}")]
    GravatarProfile(String),

    /// `https://imdb.com/name/:id/`
    #[display("https://imdb.com/name/{_0}/")]
    ImdbName(String),

    /// `https://imdb.com/title/:id/`
    #[display("https://imdb.com/title/{_0}/")]
    ImdbTitle(String),

    /// `https://instagram.com/:handle`
    #[display("https://instagram.com/{_0}")]
    InstagramProfile(String),

    /// `https://instagram.com/:handle#followers`
    #[display("https://instagram.com/{_0}#followers")]
    InstagramProfileFollowers(String),

    /// `https://instagram.com/:handle#following`
    #[display("https://instagram.com/{_0}#following")]
    InstagramProfileFollowing(String),

    /// `https://instagram.com/:handle#mutuals`
    #[cfg(feature = "unstable")] // TODO
    #[display("https://instagram.com/{_0}#mutuals")]
    InstagramProfileMutuals(String),

    /// `https://intro.co/:handle`
    #[display("https://intro.co/{_0}")]
    IntrocoProfile(String),

    /// `https://linkedin.com/in/:handle/`
    #[display("https://linkedin.com/in/{_0}/")]
    LinkedinProfile(String),

    /// `https://linkedin.com/company/:handle/`
    #[display("https://linkedin.com/company/{_0}/")]
    LinkedinCompanyPage(String),

    /// `https://luma.com/:slug` (a calendar handle or `cal-` ID)
    #[display("https://luma.com/{_0}")]
    LumaCalendar(String),

    /// `https://luma.com/:calendar?period=future`
    #[display("https://luma.com/{_0}?period=future")]
    LumaCalendarEvents(String),

    /// `https://luma.com/:calendar?period=past`
    #[display("https://luma.com/{_0}?period=past")]
    LumaCalendarPastEvents(String),

    /// `https://luma.com/:slug` (an event handle or `evt-` ID)
    /// `https://luma.com/:calendar?e=:slug`
    #[display("https://luma.com/{_0}")]
    LumaEvent(String),

    /// `https://luma.com/:slug` (an event or a calendar; resolved via the Luma API at fetch time)
    #[display("https://luma.com/{_0}")]
    LumaPage(String),

    /// `https://luma.com/user/:handle`
    #[display("https://luma.com/user/{_0}")]
    LumaProfile(String),

    /// `https://x.com/i/lists/:id`
    #[display("https://x.com/i/lists/{_0}")]
    XList(i64),

    /// `https://x.com/:handle`
    #[display("https://x.com/{_0}")]
    XProfile(String),

    /// `https://x.com/:handle/followers`
    #[display("https://x.com/{_0}/followers")]
    XProfileFollowers(String),

    /// `https://x.com/:handle/following`
    #[display("https://x.com/{_0}/following")]
    XProfileFollowing(String),

    /// `https://x.com/:handle/mutuals`
    #[cfg(feature = "unstable")] // TODO
    #[display("https://x.com/{_0}/mutuals")]
    XProfileMutuals(String),

    /// `https://x.com/:handle/highlights`
    #[display("https://x.com/{_0}/highlights")]
    XProfileHighlights(String),

    /// `https://x.com/:handle/all`
    /// `https://x.com/:handle/posts`
    /// `https://x.com/:handle#posts`
    #[display("https://x.com/{_0}/all")]
    XProfilePosts(String),

    /// `https://x.com/:handle/lists/:list`
    #[cfg(feature = "unstable")] // TODO
    #[display("https://x.com/{_0}/lists/{_1}")]
    XProfileList(String, String),
}

impl From<SocialLink> for String {
    fn from(input: SocialLink) -> Self {
        input.to_string()
    }
}

impl From<&SocialLink> for FollowRelationship {
    fn from(input: &SocialLink) -> Self {
        use FollowRelationship::*;
        use SocialLink::*;
        match input {
            GithubProfileFollowers(_) | InstagramProfileFollowers(_) | XProfileFollowers(_) => {
                Follower
            },

            GithubProfileFollowing(_) | InstagramProfileFollowing(_) | XProfileFollowing(_) => {
                Followee
            },

            #[cfg(feature = "unstable")] // TODO
            GithubProfileMutuals(_) | InstagramProfileMutuals(_) | XProfileMutuals(_) => Mutual,

            _ => unreachable!(),
        }
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
        if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
            return Err(SocialLinkError::SpuriousCredentials);
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
                if let Some(event) = query.and_then(|q| q.strip_prefix("e=")).and_then(handle) {
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
    use super::{SocialLink, SocialLinkError};
    use alloc::{format, string::ToString};

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
