// This is free and unencumbered software released into the public domain.

use alloc::string::{String, ToString};
use core::fmt;

/// Rejects names that are not portable, single filename components.
///
/// Rejects empty names, dot components, separators on either platform, control
/// characters, Windows reserved characters/device names, and trailing dots or
/// spaces. Ordinary spaces, Unicode, and interior dots are allowed. This is
/// lexical validation; filesystem access must separately constrain symlinks.
pub fn validate_filename_component(name: &str) -> Result<(), InvalidFilenameComponent> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        || stem.get(..3).is_some_and(|prefix| {
            (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
                && matches!(
                    stem.get(3..),
                    Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                )
        });
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
        || device
    {
        return Err(InvalidFilenameComponent(name.to_string()));
    }
    Ok(())
}

/// A name that cannot safely represent one portable filename component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidFilenameComponent(pub String);

impl fmt::Display for InvalidFilenameComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid filename component {:?}", self.0)
    }
}

impl core::error::Error for InvalidFilenameComponent {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nonportable_components() {
        for name in [
            "",
            ".",
            "..",
            "../secret",
            "a/../b",
            "/tmp/key",
            "a/b",
            "a\\b",
            "C:\\key",
            "C:key",
            "\\\\server\\share",
            "key:stream",
            "key\0",
            "key\n",
            "key.",
            "key ",
            "a*b",
            "a?b",
            "a|b",
            "a<b",
            "a>b",
            "a\"b",
            "CON",
            "nul.txt",
            "aux",
            "prn",
            "COM1",
            "lpt9.key",
            "LPT¹",
            "COM².txt",
            "CONIN$",
            "CONOUT$",
            "con .txt",
        ] {
            assert!(validate_filename_component(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn accepts_ordinary_names_without_normalization() {
        for name in [
            "default",
            "my-profile",
            "module_name",
            "api.key",
            "Alice Smith",
            "alice@example.org",
            "用户",
            "COM10",
            ".hidden",
        ] {
            assert!(validate_filename_component(name).is_ok(), "{name:?}");
        }
    }
}
