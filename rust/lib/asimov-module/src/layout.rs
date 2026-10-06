// This is free and unencumbered software released into the public domain.

//! Shared on-disk module layout, relative to the ASIMOV state root.

/// Directory containing installed and enabled module collections.
pub const MODULES_DIR_NAME: &str = "modules";
/// Installed modules live in `<modules>/installed/<name>/`.
pub const INSTALLED_DIR_NAME: &str = "installed";
/// Manifest filename within an installed module directory.
pub const MANIFEST_FILE_NAME: &str = "manifest.json";

/// Prefer the installed directory, then legacy JSON, YAML, and YML files.
/// Open each directory separately to preserve capability boundaries.
#[cfg(all(feature = "std", feature = "serde"))]
pub(crate) fn read_manifest_bytes(
    directory: &asimov_core::crates::cap_std::fs::Dir,
    name: &str,
) -> std::io::Result<Option<(std::path::PathBuf, alloc::vec::Vec<u8>)>> {
    use alloc::format;
    use std::{io::ErrorKind, path::PathBuf};
    let paths = [
        PathBuf::from(name).join(MANIFEST_FILE_NAME),
        PathBuf::from(format!("{name}.json")),
        PathBuf::from(format!("{name}.yaml")),
        PathBuf::from(format!("{name}.yml")),
    ];
    for path in paths {
        let content = match path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            Some(parent) => directory
                .open_dir(parent)
                .and_then(|dir| dir.read(path.file_name().unwrap())),
            None => directory.read(&path),
        };
        match content {
            Ok(content) => return Ok(Some((path, content))),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}
