// This is free and unencumbered software released into the public domain.

// The same tests cover builds where `SocialHandle` has no enabled variants.
#![allow(unreachable_code, unused_variables)]

use crate::{
    SocialHandle, SocialLink, SocialLinkConversionError, SocialPlatform, SocialPlatformHandleError,
};
use alloc::{format, string::ToString};

#[test]
fn every_platform_has_round_tripping_profiles() {
    use SocialPlatform::*;
    // Link constructors are deliberately independent of platform feature flags.
    let cases: &[(
        SocialPlatform,
        &str,
        fn(alloc::string::String) -> SocialLink,
    )] = &[
        (Bluesky, "alice.bsky.social", SocialLink::BlueskyProfile),
        (Discord, "123456789012345678", SocialLink::DiscordProfile),
        (Facebook, "alice", SocialLink::FacebookProfile),
        (Github, "alice", SocialLink::GithubProfile),
        (Gitlab, "alice", SocialLink::GitlabProfile),
        (Gravatar, "alice", SocialLink::GravatarProfile),
        (Instagram, "alice", SocialLink::InstagramProfile),
        (Introco, "alice", SocialLink::IntrocoProfile),
        (Linkedin, "alice", SocialLink::LinkedinProfile),
        (Localai, "alice", SocialLink::LocalaiProfile),
        (Luma, "alice", SocialLink::LumaProfile),
        (Medium, "alice", SocialLink::MediumProfile),
        (Pinterest, "alice", SocialLink::PinterestProfile),
        (Reddit, "alice", SocialLink::RedditProfile),
        (Snapchat, "alice", SocialLink::SnapchatProfile),
        (Substack, "alice", SocialLink::SubstackProfile),
        (Telegram, "alice", SocialLink::TelegramProfile),
        (Threads, "alice", SocialLink::ThreadsProfile),
        (Tiktok, "alice", SocialLink::TiktokProfile),
        (Twitch, "alice", SocialLink::TwitchProfile),
        (Whatsapp, "alice", SocialLink::WhatsappProfile),
        (X, "alice", SocialLink::XProfile),
        (Youtube, "alice", SocialLink::YoutubeProfile),
    ];
    for &(platform, identifier, constructor) in cases {
        let link = constructor(identifier.into());
        let canonical = link.to_string();
        let www = canonical.replacen("https://", "https://www.", 1);
        let authority = format!(
            "{}:443",
            platform
                .to_string()
                .replacen("https://", "https://www.", 1)
                .to_ascii_uppercase()
        );
        let normalized_input = canonical.replacen(
            &format!("{platform}/"),
            &format!("{authority}/discard/../"),
            1,
        );
        assert_eq!(canonical.parse::<SocialLink>().unwrap(), link);
        assert_eq!(www.parse::<SocialLink>().unwrap(), link);
        assert_eq!(authority.parse::<SocialPlatform>().unwrap(), platform);
        assert_eq!(normalized_input.parse::<SocialLink>().unwrap(), link);
        assert!(!canonical.contains("www."));
        for suffix in ["?unexpected=1", "#unexpected"] {
            assert!(format!("{www}{suffix}").parse::<SocialLink>().is_err());
        }
        assert!(canonical.starts_with(&format!(
            "{}/{}{}",
            platform,
            platform.handle_prefix().unwrap_or(""),
            identifier
        )));

        match platform.handle(identifier) {
            Ok(handle) => {
                assert_eq!(handle.as_str(), identifier);
                assert_eq!(handle.to_string(), canonical);
                #[cfg(feature = "async-graphql")]
                {
                    use async_graphql::connection::CursorType;
                    assert_eq!(handle.encode_cursor(), canonical);
                    assert_eq!(SocialHandle::decode_cursor(&www).unwrap(), handle);
                }
                assert_eq!(www.parse::<SocialHandle>().unwrap(), handle);
                assert_eq!(normalized_input.parse::<SocialHandle>().unwrap(), handle);
                assert_eq!(canonical.parse::<SocialHandle>().unwrap(), handle);
                assert_eq!(SocialLink::try_from(&handle).unwrap(), link);
                assert_eq!(SocialLink::try_from(handle.clone()).unwrap(), link);
                assert_eq!(SocialHandle::try_from(&link).unwrap(), handle);
                assert_eq!(SocialHandle::try_from(link).unwrap(), handle);
                assert!(matches!(
                    platform.handle(""),
                    Err(SocialPlatformHandleError::InvalidHandle(_))
                ));
                for suffix in ["/extra", "?unexpected=1", "#unexpected"] {
                    assert!(format!("{www}{suffix}").parse::<SocialHandle>().is_err());
                }
            },
            Err(SocialPlatformHandleError::DisabledPlatform(disabled)) => {
                assert_eq!(disabled, platform);
                assert!(www.parse::<SocialHandle>().is_err());
                assert!(normalized_input.parse::<SocialHandle>().is_err());
                assert!(matches!(
                    SocialHandle::try_from(link),
                    Err(SocialLinkConversionError::UnsupportedLink)
                ));
            },
            Err(error) => panic!("{platform}: {error}"),
        }
    }
}

#[test]
fn url_authority_validation_is_shared() {
    for base in [
        "http://www.reddit.com",
        "https://www.reddit.com:8443",
        "https://www.reddit.com:not-a-port",
        "https://www.reddit.com.evil.example",
        "https://www.reddit.com@evil.example",
        "https://user@www.reddit.com",
        "https://:secret@www.reddit.com",
        "https://www.www.reddit.com",
    ] {
        assert!(base.parse::<SocialPlatform>().is_err(), "{base}");
        let profile = format!("{base}/user/alice");
        assert!(profile.parse::<SocialHandle>().is_err(), "{profile}");
        assert!(profile.parse::<SocialLink>().is_err(), "{profile}");
    }
}

#[test]
fn new_link_forms_reject_wrong_resource_selectors() {
    for input in [
        "https://reddit.com/r/rust",
        "https://reddit.com/user/",
        "https://reddit.com/user/alice/posts",
        "https://pinterest.com/alice/board",
        "https://snapchat.com/alice",
        "https://snapchat.com/add/",
        "https://twitch.tv/alice/videos",
        "https://discord.com/channels/123",
        "https://discord.com/users/",
        "https://youtube.com/channel/UC123",
        "https://bsky.app/profile/",
        "https://substack.com/@",
        "https://medium.com/@",
        "https://threads.com/@",
        "https://www.www.reddit.com/user/alice",
        "https://www.reddit.com.evil.example/user/alice",
        "https://user@www.reddit.com/user/alice",
        "https://www.reddit.com:8443/user/alice",
    ] {
        assert!(input.parse::<SocialLink>().is_err(), "{input}");
        assert!(input.parse::<SocialHandle>().is_err(), "{input}");
    }
}

#[test]
fn whatsapp_phone_links_are_not_usernames() {
    let link: SocialLink = "https://www.wa.me/14155552671".parse().unwrap();
    assert_eq!(link, SocialLink::WhatsappContact("14155552671".into()));
    assert_eq!(link.to_string(), "https://wa.me/14155552671");
    assert!(matches!(
        SocialHandle::try_from(link),
        Err(SocialLinkConversionError::UnsupportedLink)
    ));
}

#[test]
fn profile_aliases_canonicalize() {
    for (input, canonical) in [
        (
            "https://www.reddit.com/u/alice",
            "https://reddit.com/user/alice",
        ),
        (
            "https://www.threads.com/@alice",
            "https://threads.net/@alice",
        ),
    ] {
        assert_eq!(input.parse::<SocialLink>().unwrap().to_string(), canonical);
        if let Ok(handle) = canonical.parse::<SocialHandle>() {
            assert_eq!(input.parse::<SocialHandle>().unwrap(), handle);
            assert_eq!(handle.to_string(), canonical);
        }
    }
    assert_eq!(
        "https://www.threads.com/"
            .parse::<SocialPlatform>()
            .unwrap(),
        SocialPlatform::Threads
    );
}

#[cfg(feature = "discord")]
#[test]
fn discord_uses_snowflakes_not_usernames() {
    for invalid in [
        "alice",
        "0",
        "01",
        "-1",
        "+1",
        "18446744073709551616",
        "1.0",
    ] {
        assert!(SocialHandle::discord(invalid).is_err(), "{invalid}");
        assert!(matches!(
            SocialHandle::try_from(SocialLink::DiscordProfile(invalid.into())),
            Err(SocialLinkConversionError::InvalidHandle(_))
        ));
    }
    assert!(SocialHandle::discord("18446744073709551615").is_ok());
}

#[test]
fn username_validation_and_percent_decoding() {
    for (platform, invalid) in [
        (SocialPlatform::Pinterest, "contains-dash"),
        (SocialPlatform::Pinterest, "ab"),
        (SocialPlatform::Reddit, "ab"),
        (SocialPlatform::Reddit, "contains.dot"),
        (SocialPlatform::Snapchat, "1alice"),
        (SocialPlatform::Snapchat, "alice_"),
        (SocialPlatform::Twitch, "abc"),
        (SocialPlatform::Twitch, "contains.dot"),
    ] {
        assert!(platform.handle(invalid).is_err(), "{platform}: {invalid}");
    }
    for platform in [
        SocialPlatform::Pinterest,
        SocialPlatform::Reddit,
        SocialPlatform::Snapchat,
        SocialPlatform::Twitch,
    ] {
        if let Ok(handle) = platform.handle("alice") {
            assert_eq!(platform.handle("%61lice").unwrap(), handle);
            for invalid in [
                "%",
                "%GG",
                "%FF",
                "alice%2Fextra",
                "alice%3Fquery",
                "alice%23fragment",
            ] {
                assert!(platform.handle(invalid).is_err(), "{platform}: {invalid}");
            }
        }
    }
}

#[cfg(feature = "youtube")]
#[test]
fn unicode_handles_round_trip_through_urls() {
    let handle = SocialHandle::youtube("日本語").unwrap();
    let url = handle.to_string();
    assert!(url.is_ascii());
    let link: SocialLink = url.parse().unwrap();
    assert_eq!(SocialLink::try_from(&handle).unwrap(), link);
    assert_eq!(SocialHandle::try_from(link).unwrap(), handle);
    assert_eq!(url.parse::<SocialHandle>().unwrap(), handle);
}

#[cfg(feature = "bluesky")]
#[test]
fn bluesky_dids_are_links_not_domain_handles() {
    let link: SocialLink = "https://www.bsky.app/profile/did:plc:abc".parse().unwrap();
    assert!(matches!(
        SocialHandle::try_from(link),
        Err(SocialLinkConversionError::InvalidHandle(_))
    ));
    for invalid in [
        "alice",
        "alice..social",
        "-alice.social",
        "alice-.social",
        "alice.123",
    ] {
        assert!(SocialHandle::bluesky(invalid).is_err());
    }
}
