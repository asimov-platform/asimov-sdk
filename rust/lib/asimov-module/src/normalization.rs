// This is free and unencumbered software released into the public domain.

use alloc::string::{String, ToString};
use iri_string::types::{IriReferenceStr, IriReferenceString};

/// Normalizes module names by removing dots and converting to lowercase.
/// Allows domain names like `near.ai` or names stylized with capital letters.
/// The `name` field of [`crate::ModuleManifest`] should equal the normalized form.
///
/// # Examples
///
/// ```
/// # use asimov_module::normalization::normalize_module_name;
/// assert_eq!(normalize_module_name("foo.bar"), "foobar");
/// assert_eq!(normalize_module_name("FOOBAR"), "foobar");
/// ```
pub fn normalize_module_name(module: &str) -> String {
    module.replace('.', "").to_lowercase()
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum NormalizeError {
    #[error(transparent)]
    Parse(#[from] iri_string::types::CreationError<String>),
    #[error(transparent)]
    Build(#[from] iri_string::validate::Error),
}

/// Normalizes URLs and file paths into valid IRI format with consistent scheme handling.
///
/// Adds `file:` scheme to paths, resolves relative paths, handles `~/` expansion,
/// and properly encodes spaces and special characters.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), Box<dyn core::error::Error>> {
/// # use asimov_module::normalization::normalize_url;
/// assert_eq!(normalize_url("https://example.org")?, "https://example.org/");
/// assert!(normalize_url("path with spaces.txt")?.starts_with("file:"));
/// assert!(normalize_url("~/document.txt")?.ends_with("/document.txt"));
/// # Ok(())
/// # }
/// ```
pub fn normalize_url(url: &str) -> Result<String, NormalizeError> {
    let iri = IriReferenceString::try_from(url)
        .or_else(|_| IriReferenceString::try_from(url.replace(" ", "%20")))?;

    let mut builder = iri_string::build::Builder::new();

    // default `file:` scheme
    let scheme = iri.scheme_str().unwrap_or("file");
    builder.scheme(scheme);

    if let Some(auth) = iri.authority_components() {
        if let Some(user) = auth.userinfo() {
            builder.userinfo(user);
        }
        builder.host(auth.host());
        if let Some(port) = auth.port() {
            builder.port(port);
        }
    }

    let path = iri.path_str();

    // TODO: utilize `path.normalize_lexically()` once it stabilizes
    // https://github.com/rust-lang/rust/issues/134694

    #[cfg(feature = "std")]
    let path = if scheme == "file" && path.starts_with("~/") {
        let rest = path.strip_prefix("~/").unwrap(); // safe, the prefix was just checked just

        let home_dir = std::env::home_dir().expect("unable to determine home directory");

        let path = home_dir.join(rest);
        let path = std::path::absolute(&path).unwrap_or(path);
        let path = path.canonicalize().unwrap_or(path);

        path.display().to_string()
    } else if scheme == "file" {
        // `std::path::absolute` also changes relative paths to absolute with the current directory
        // as base.
        let path = std::path::absolute(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
        let path = path.canonicalize().unwrap_or(path);

        path.display().to_string()
    } else if iri.authority_str().is_some() && path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };

    #[cfg(windows)]
    let path = if scheme == "file" && !path.starts_with("/") {
        "/".to_string() + &path.replace('\\', "/")
    } else {
        path.to_string()
    };

    builder.path(&path);

    if let Some(query) = iri.query() {
        builder.query(query.as_str());
    }

    if let Some(fraq) = iri.fragment() {
        builder.fragment(fraq.as_str());
    }

    builder.normalize();

    builder
        .build::<IriReferenceStr>()
        .map(|r| r.to_string())
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{format, string::ToString};

    #[test]
    fn url_normalization() {
        let cases = [
            ("https://example.org", "https://example.org/"),
            ("https://example.org/", "https://example.org/"),
            ("http://example.com/path", "http://example.com/path"),
            ("https://api.example.com", "https://api.example.com/"),
            ("http://localhost:3000", "http://localhost:3000/"),
            ("ftp://fileserver.local", "ftp://fileserver.local/"),
            (
                "https://user:pass@example.org:8080/path?foo=bar&query=hello world#fragment",
                "https://user:pass@example.org:8080/path?foo=bar&query=hello%20world#fragment",
            ),
            ("near://testnet/123456789", "near://testnet/123456789"),
            (
                "ftp://files.example.com/file.txt",
                "ftp://files.example.com/file.txt",
            ),
            ("ws://localhost:3000/socket", "ws://localhost:3000/socket"),
            ("mailto:user@example.com", "mailto:user@example.com"),
            (
                "https://example.org/path with spaces",
                "https://example.org/path%20with%20spaces",
            ),
            (
                "https://example.org/path+with+plus",
                "https://example.org/path+with+plus",
            ),
            (
                "https://example.org/path%20already%20encoded",
                "https://example.org/path%20already%20encoded",
            ),
            (
                "data:text/plain;base64,SGVsbG8=",
                "data:text/plain;base64,SGVsbG8=",
            ),
            ("tel:+1-555-123-4567", "tel:+1-555-123-4567"),
            ("urn:isbn:1234567890", "urn:isbn:1234567890"),
            (
                "ldap://[2001:db8::7]/c=GB?objectClass?one",
                "ldap://[2001:db8::7]/c=GB?objectClass?one",
            ),
            (
                "ldap://foo:bar@[2001:db8::7]:80/c=GB?objectClass?one",
                "ldap://foo:bar@[2001:db8::7]:80/c=GB?objectClass?one",
            ),
            ("telnet://192.0.2.16:80", "telnet://192.0.2.16:80/"),
            // TODO: should this be inferred?
            // ("localhost:8080", "http://localhost:8080"),
        ];

        for case in cases {
            assert_eq!(
                normalize_url(case.0).expect(case.0),
                case.1,
                "input: {:?}",
                case.0
            );
        }

        #[cfg(unix)]
        {
            let cases = [
                ("/file with spaces.txt", "file:/file%20with%20spaces.txt"),
                ("/file+with+pluses.txt", "file:/file+with+pluses.txt"),
                (
                    // Plain strings get `file:` scheme and current directory prepended
                    "document.txt",
                    &format!(
                        "file:{}/document.txt",
                        std::env::current_dir().unwrap().display()
                    ),
                ),
                (
                    // Domain-like strings without scheme get treated as files
                    "example.org",
                    &format!(
                        "file:{}/example.org",
                        std::env::current_dir().unwrap().display()
                    ),
                ),
                (
                    "folder name/file.txt",
                    &format!(
                        "file:{}/folder%20name/file.txt",
                        std::env::current_dir().unwrap().display()
                    ),
                ),
                (
                    "./subfolder/../file.txt",
                    &format!(
                        "file:{}/file.txt",
                        std::env::current_dir().unwrap().display()
                    ),
                ),
                (
                    "../parent/./file.txt",
                    &format!(
                        "file:{}/parent/file.txt",
                        std::env::current_dir().unwrap().parent().unwrap().display()
                    ),
                ),
            ];

            for case in cases {
                assert_eq!(
                    normalize_url(case.0).unwrap(),
                    case.1,
                    "input: {:?}",
                    case.0
                );
            }

            if let Some(home_dir) = std::env::home_dir() {
                let home_dir = home_dir.display().to_string();

                let input = "~/path/to/file.txt";
                let want = "file:".to_string() + &home_dir + "/path/to/file.txt";
                assert_eq!(
                    normalize_url(input).unwrap(),
                    want,
                    "relative path should be get added after current directory, input: {:?}",
                    input
                );
            }
        }

        #[cfg(windows)]
        {
            let cwd = std::env::current_dir().unwrap();
            let drive = cwd.to_str().unwrap().chars().next().unwrap();
            let cases = [
                (
                    "/file with spaces.txt",
                    format!("file:/{drive}:/file%20with%20spaces.txt"),
                ),
                (
                    "/file+with+pluses.txt",
                    format!("file:/{drive}:/file+with+pluses.txt"),
                ),
            ];

            for case in cases {
                assert_eq!(
                    normalize_url(case.0).unwrap(),
                    case.1,
                    "input: {:?}",
                    case.0
                );
            }
        }
    }
}
