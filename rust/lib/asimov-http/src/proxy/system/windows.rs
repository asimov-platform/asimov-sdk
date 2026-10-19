// This is free and unencumbered software released into the public domain.

//! Windows static Internet Settings. Read the documented registry values so an
//! enabled static proxy remains usable alongside an unevaluated PAC/WPAD setup.

use super::{Environment, Error, Route, native};
use url::Url;
use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};

const SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
const POLICY: &str = r"Software\Policies\Microsoft\Windows\CurrentVersion\Internet Settings";

fn optional<T>(
    result: windows_registry::Result<T>,
    setting: &'static str,
) -> Result<Option<T>, Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND).
        Err(error) if error.code().0 as u32 == 0x8007_0002 => Ok(None),
        Err(_) => Err(Error::InvalidProxy(setting)),
    }
}

pub(super) fn read(url: &Url, env: &Environment) -> Result<Route, Error> {
    let policy = optional(LOCAL_MACHINE.open(POLICY), "ProxySettingsPerUser")?;
    let per_user = policy
        .as_ref()
        .map(|key| optional(key.get_u32("ProxySettingsPerUser"), "ProxySettingsPerUser"))
        .transpose()?
        .flatten();
    let root = if per_user == Some(0) {
        LOCAL_MACHINE
    } else {
        CURRENT_USER
    };
    match optional(root.open(SETTINGS), "Internet Settings")? {
        Some(key) => read_key(&key, url, env),
        None => Ok(Route::Automatic),
    }
}

fn read_key(key: &Key, url: &Url, env: &Environment) -> Result<Route, Error> {
    if optional(key.get_u32("ProxyEnable"), "ProxyEnable")? != Some(1) {
        return Ok(Route::Automatic);
    }
    let server = optional(key.get_string("ProxyServer"), "ProxyServer")?
        .filter(|value| !value.trim().is_empty())
        .ok_or(Error::InvalidProxy("ProxyServer"))?;
    let bypass = optional(key.get_string("ProxyOverride"), "ProxyOverride")?.unwrap_or_default();
    let mode = proxy_watch::parse::windows_manual(&server, &bypass);
    native::route(&mode, url, env, "ProxyServer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn reads_static_settings_from_an_isolated_registry_key() {
        let path = format!(r"Software\ASIMOV\Tests\Proxy-{}", std::process::id());
        struct Cleanup(alloc::string::String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = CURRENT_USER.remove_tree(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        let key = CURRENT_USER.create(&path).unwrap();
        key.set_u32("ProxyEnable", 1).unwrap();
        key.set_string(
            "ProxyServer",
            "http=web.invalid:8080;https=tunnel.invalid:3128",
        )
        .unwrap();
        key.set_string("ProxyOverride", "<local>;*.corp.invalid")
            .unwrap();
        // Automatic settings must not hide the enabled static fallback.
        key.set_u32("AutoDetect", 1).unwrap();
        key.set_string("AutoConfigURL", "http://pac.invalid/proxy.pac")
            .unwrap();
        let env = Environment::default();
        assert_eq!(
            read_key(&key, &Url::parse("https://service.invalid").unwrap(), &env).unwrap(),
            Route::Native(Url::parse("http://tunnel.invalid:3128").unwrap())
        );
        assert_eq!(
            read_key(&key, &Url::parse("http://intranet").unwrap(), &env).unwrap(),
            Route::Direct
        );
        key.set_u32("ProxyEnable", 0).unwrap();
        key.set_string("ProxyServer", "http://User:Secret@proxy.invalid:bad")
            .unwrap();
        assert_eq!(
            read_key(&key, &Url::parse("http://service.invalid").unwrap(), &env).unwrap(),
            Route::Automatic
        );
        key.set_u32("ProxyEnable", 1).unwrap();
        assert!(matches!(
            read_key(&key, &Url::parse("http://service.invalid").unwrap(), &env),
            Err(Error::InvalidProxy("ProxyServer"))
        ));
    }
}
