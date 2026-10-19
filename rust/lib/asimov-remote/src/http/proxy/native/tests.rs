// This is free and unencumbered software released into the public domain.

use super::*;
use proxy_watch::parse::windows_manual;

fn windows(server: &str, bypass: &str, url: &str, no_proxy: &str) -> Result<Route, Error> {
    route(
        &windows_manual(server, bypass),
        &Url::parse(url).unwrap(),
        &Environment {
            no_proxy: no_proxy.into(),
            ..Environment::default()
        },
        "ProxyServer",
    )
}

#[test]
fn windows_protocol_maps_select_destination_schemes_not_proxy_tls() {
    for (server, url, expected) in [
        (
            "proxy.invalid:3128",
            "https://service.invalid",
            "http://proxy.invalid:3128/",
        ),
        (
            "http=web.invalid:8080;https=secure.invalid:8443",
            "https://service.invalid",
            "http://secure.invalid:8443/",
        ),
        (
            "http=web.invalid:8080 https=secure.invalid",
            "http://service.invalid",
            "http://web.invalid:8080/",
        ),
        (
            "https=https://tls.invalid:8443",
            "https://service.invalid",
            "https://tls.invalid:8443/",
        ),
        (
            "socks=socks.invalid:1080",
            "https://service.invalid",
            "socks4://socks.invalid:1080",
        ),
        (
            "http=web.invalid:8080;socks=socks5h://socks.invalid:1080",
            "https://service.invalid",
            "socks5h://socks.invalid:1080",
        ),
        (
            "http=web.invalid:8080;socks=socks.invalid:1080",
            "http://service.invalid",
            "http://web.invalid:8080/",
        ),
        (
            "https=[2001:db8::1]:3128",
            "https://service.invalid",
            "http://[2001:db8::1]:3128/",
        ),
        (
            "https=http://User%40Realm:p%3Aa%25ss+@proxy.invalid:3128",
            "https://service.invalid",
            "http://User%40Realm:p%3Aa%25ss+@proxy.invalid:3128/",
        ),
    ] {
        assert_eq!(
            windows(server, "", url, "").unwrap(),
            Route::Native(Url::parse(expected).unwrap()),
            "{url} via {server}"
        );
    }
    for server in ["http=web.invalid:8080", "https=", ""] {
        assert_eq!(
            windows(server, "", "https://service.invalid", "").unwrap(),
            Route::Automatic
        );
    }
}

#[test]
fn windows_exclusions_keep_their_native_grammar() {
    for (bypass, destination, excluded) in [
        ("<local>", "intranet", true),
        ("<LOCAL>", "intranet", true),
        ("<local>", "intranet.invalid", false),
        ("<local>", "192.0.2.1", false),
        ("<local>", "[2001:db8::1]", false),
        ("corp.invalid", "a.corp.invalid", false),
        ("corp.invalid", "corp.invalid", true),
        ("*.corp.invalid", "corp.invalid", false),
        ("*.corp.invalid", "a.corp.invalid", true),
        ("192.168.*", "192.168.1.2", true),
        ("*.other.invalid;CORP.INVALID", "corp.invalid", true),
        ("intranet *.corp.invalid", "a.corp.invalid", true),
        ("corp.invalid:8080", "corp.invalid:8080", true),
        ("corp.invalid:8080", "corp.invalid:80", false),
        ("<-loopback>", "127.0.0.1", false),
        ("", "127.0.0.1", true),
    ] {
        let url = alloc::format!("http://{destination}");
        let selected = windows("proxy.invalid:3128", bypass, &url, "").unwrap();
        assert_eq!(
            selected == Route::Direct,
            excluded,
            "{bypass:?} against {url}"
        );
        // Nonempty environment rules replace, rather than merge, native rules.
        assert!(matches!(
            windows("proxy.invalid:3128", bypass, &url, "other.invalid"),
            Ok(Route::Native(_))
        ));
    }
}

#[test]
fn unusable_native_endpoints_error_without_disclosing_credentials() {
    for server in [
        "https=http://User:Secret@proxy.invalid:99999",
        "https=ftp://User:Secret@proxy.invalid",
        "https=http://User:Secret@[bad]",
    ] {
        let error = windows(server, "", "https://service.invalid", "").unwrap_err();
        assert!(matches!(error, Error::InvalidProxy("ProxyServer")));
        let text = alloc::format!("{error:?}: {error}");
        assert!(!text.contains("Secret"));
        assert!(!text.contains("User"));
        // A malformed setting for another destination scheme is irrelevant.
        assert_eq!(
            windows(server, "", "http://service.invalid", "").unwrap(),
            Route::Automatic
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_desktop_settings_are_read_from_isolated_native_stores() {
    const CHILD: &str = "ASIMOV_LINUX_PROXY_TEST";
    if let Ok(case) = std::env::var(CHILD) {
        let env = Environment::default();
        let public = Url::parse("https://service.invalid").unwrap();
        let private = Url::parse("https://api.corp.invalid").unwrap();
        let expected = match case.as_str() {
            "gnome" => "http://gnome.invalid:3128/",
            "kde" | "kde-reversed" => "http://kde.invalid:8080/",
            "kde-system" => "http://system.invalid:3128/",
            "socks" => "socks5h://socks.invalid:1080",
            _ => unreachable!(),
        };
        assert_eq!(
            read_linux(&public, &env).unwrap(),
            Route::Native(Url::parse(expected).unwrap())
        );
        assert_eq!(read_linux(&private, &env).unwrap(), Route::Direct);
        let env = Environment {
            no_proxy: "other.invalid".into(),
            ..env
        };
        assert!(matches!(
            read_linux(&private, &env).unwrap(),
            Route::Native(_)
        ));
        return;
    }
    let schemas = tempfile::tempdir().unwrap();
    std::fs::write(
        schemas.path().join("proxy.gschema.xml"),
        include_str!("../../../../tests/fixtures/proxy.gschema.xml"),
    )
    .unwrap();
    let gnome_available = match std::process::Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg(schemas.path())
        .output()
    {
        Ok(output) => {
            assert!(output.status.success(), "{output:?}");
            true
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::eprintln!("GNOME fixtures require glib-compile-schemas; exercising KDE only");
            false
        },
        Err(error) => panic!("{error}"),
    };
    for case in ["gnome", "kde", "socks", "kde-reversed", "kde-system"] {
        if !case.starts_with("kde") && !gnome_available {
            continue;
        }
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        std::fs::create_dir_all(config.join("glib-2.0/settings")).unwrap();
        let keyfile = match case {
            "gnome" => {
                "[system/proxy]\nmode='manual'\nuse-same-proxy=true\nignore-hosts=['.corp.invalid']\n[system/proxy/http]\nhost='gnome.invalid'\nport=3128\n"
            },
            "socks" => {
                "[system/proxy]\nmode='manual'\nuse-same-proxy=false\nignore-hosts=['.corp.invalid']\n[system/proxy/socks]\nhost='socks.invalid'\nport=1080\n"
            },
            _ => "[system/proxy]\nmode='none'\n",
        };
        std::fs::write(config.join("glib-2.0/settings/keyfile"), keyfile).unwrap();
        // KDE must take precedence over GNOME's explicit mode=none.
        if case.starts_with("kde") {
            let bypass = if case == "kde-reversed" {
                "NoProxyFor=service.invalid\nReversedException=true\n"
            } else {
                "NoProxyFor=.corp.invalid\n"
            };
            std::fs::write(
                config.join("kioslaverc"),
                alloc::format!(
                    "[Proxy Settings]\nProxyType=1\nhttpsProxy=http://kde.invalid 8080\n{bypass}"
                ),
            )
            .unwrap();
        }
        if case == "kde-system" {
            std::fs::create_dir(root.path().join("system")).unwrap();
            std::fs::write(
                root.path().join("system/kioslaverc"),
                "[Proxy Settings]\nhttpsProxy[$i]=http://system.invalid:3128\n",
            )
            .unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "http::proxy::native::tests::linux_desktop_settings_are_read_from_isolated_native_stores", "--nocapture"])
            .env(CHILD, case)
            .env("HOME", root.path())
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CONFIG_DIRS", root.path().join("system"))
            .env("XDG_CURRENT_DESKTOP", if case.starts_with("kde") { "KDE" } else { "GNOME" })
            .env("GSETTINGS_BACKEND", "keyfile")
            .env("GSETTINGS_SCHEMA_DIR", schemas.path())
            .env_remove("SNAP")
            .output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            alloc::string::String::from_utf8_lossy(&output.stdout),
            alloc::string::String::from_utf8_lossy(&output.stderr)
        );
    }
}
