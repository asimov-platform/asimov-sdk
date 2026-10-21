// This is free and unencumbered software released into the public domain.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use jiff::Timestamp;
use std::{
    io::{Result, Write},
    path::Path,
};

use crate::Snapshot;

const TIMESTAMP_FORMAT_STRING: &str = "%Y%m%dT%H%M%SZ";

/// Filesystem snapshots with the original IRI in an LF-terminated `.uri` file.
pub struct Fs {
    root: cap_std::fs::Dir,
}

impl Fs {
    pub fn new(root: cap_std::fs::Dir) -> Self {
        Self { root }
    }

    pub fn for_dir(path: std::path::PathBuf) -> Result<Self> {
        let root = cap_std::fs::Dir::open_ambient_dir(&path, cap_std::ambient_authority())?;
        Ok(Self::new(root))
    }
}

impl super::Storage for Fs {
    #[tracing::instrument(skip_all, fields(url = snapshot.url))]
    fn save_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        let url_hash = hex::encode(sha256(&snapshot.url));
        let final_url_dir = std::path::Path::new(&url_hash);

        tracing::debug!(hash = url_hash, "Creating temporary directory for writing");
        let tmp_dir = self.root.open_dir(".tmp").or_else(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                self.root.create_dir(".tmp")?;
                self.root.open_dir(".tmp")
            },
            _ => Err(err),
        })?;
        let tmp_dir = cap_tempfile::tempdir_in(&tmp_dir)?;

        tracing::debug!(hash = url_hash, "Creating directory for url");
        self.root.create_dir_all(&final_url_dir)?;

        let url_path = final_url_dir.join(".uri");
        if !self
            .root
            .metadata(&url_path)
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            tracing::debug!(path = ?url_path, "Creating `.uri` metadata file");
            let mut url_file = self.root.create(&url_path)?;
            writeln!(url_file, "{}", snapshot.url)?;

            tracing::debug!("Setting `.uri` metadata file permissions");
            let mut permissions = url_file.metadata()?.permissions();
            permissions.set_readonly(true);
            url_file.set_permissions(permissions)?;
        }

        let ts = snapshot
            .start_timestamp
            .strftime(TIMESTAMP_FORMAT_STRING)
            .to_string();
        let tmp_snapshot_dir_path = std::path::PathBuf::from(&ts);

        tracing::debug!("Creating snapshot directory");
        tmp_dir.create_dir(&tmp_snapshot_dir_path)?;

        {
            tracing::debug!("Writing snapshot data file");
            let snapshot_path = tmp_snapshot_dir_path.join("data");
            let mut snapshot_file = tmp_dir.create(snapshot_path)?;
            snapshot_file.write_all(snapshot.data.as_ref())?;

            tracing::debug!("Setting snapshot data file permissions");
            let mut permissions = snapshot_file.metadata()?.permissions();
            permissions.set_readonly(true);
            snapshot_file.set_permissions(permissions)?;
        }

        if let Some(end_ts) = snapshot.end_timestamp {
            let end_ts_path = tmp_snapshot_dir_path.join("end-timestamp");
            tracing::debug!("Writing snapshot end_timestamp file");
            let mut end_ts_file = tmp_dir.create(&end_ts_path)?;
            end_ts_file.write_all(
                end_ts
                    .strftime(TIMESTAMP_FORMAT_STRING)
                    .to_string()
                    .as_bytes(),
            )?;

            tracing::debug!("Setting snapshot end_timestamp file permissions");
            let mut permissions = end_ts_file.metadata()?.permissions();
            permissions.set_readonly(true);
            end_ts_file.set_permissions(permissions)?;
        }

        let final_snapshot_dir_path = final_url_dir.join(&ts);
        tracing::debug!("Creating final snapshot directory");
        self.root.create_dir_all(&final_snapshot_dir_path)?;
        tracing::debug!("Moving snapshot directory to final location");
        tmp_dir.rename(&tmp_snapshot_dir_path, &self.root, &final_snapshot_dir_path)?;

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(url = url.as_ref()))]
    fn read(&self, url: impl AsRef<str>, timestamp: Timestamp) -> Result<Snapshot> {
        let url_hash = hex::encode(sha256(url.as_ref()));

        let ts = timestamp.strftime(TIMESTAMP_FORMAT_STRING).to_string();
        let snapshot_path = std::path::Path::new(&url_hash).join(ts);

        let file_path = snapshot_path.join("data");

        tracing::debug!("Reading snapshot");
        let data = self.root.read(file_path)?;

        let end_ts_path = snapshot_path.join("end-timestamp");
        let end_timestamp = if self.root.exists(&end_ts_path) {
            let content = self.root.read_to_string(&end_ts_path)?;
            content
                .parse::<Timestamp>()
                .map_err(|e| std::io::Error::other(format!("Invalid timestamp `{content}`: {e}")))?
                .into()
        } else {
            None
        };

        Ok(Snapshot {
            url: url.as_ref().into(),
            start_timestamp: timestamp,
            end_timestamp,
            data,
        })
    }

    #[tracing::instrument(skip(self), fields(url = url.as_ref()))]
    fn set_current_version(&self, url: impl AsRef<str>, timestamp: Timestamp) -> Result<()> {
        let url_hash = hex::encode(sha256(url.as_ref()));
        let url_dir = std::path::Path::new(&url_hash);

        let current_link_path = url_dir.join("current");

        tracing::debug!(source = ?current_link_path, "Removing old `current` symlink");
        self.delete_current_version(&url)?;

        let ts = timestamp.strftime(TIMESTAMP_FORMAT_STRING).to_string();

        tracing::debug!(source = ?current_link_path, target = ts, "Creating new `current` symlink");

        #[cfg(unix)]
        return self.root.symlink(ts, current_link_path);

        #[cfg(windows)]
        return self.root.symlink_file(ts, current_link_path);
    }

    #[tracing::instrument(skip(self), fields(url = url.as_ref()))]
    fn current_version(&self, url: impl AsRef<str>) -> Result<Timestamp> {
        let url_hash = hex::encode(sha256(url.as_ref()));

        let link_path = std::path::Path::new(&url_hash).join("current");

        tracing::debug!(path = ?link_path, "Reading `current` symlink");
        let current = self.root.read_link(link_path)?;

        let stem = current
            .file_stem()
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "Malformed file: `{}` does not have a valid file stem",
                    current.display()
                ))
            })?
            .to_string_lossy();

        stem.parse()
            .map_err(|e| std::io::Error::other(format!("Invalid timestamp `{stem}`: {e}")))
    }

    #[tracing::instrument(skip(self))]
    fn list_urls(&self) -> Result<Vec<(String, Timestamp)>> {
        let mut urls = Vec::new();

        let read_dir = self.root.read_dir("./")?;
        for entry in read_dir {
            let entry = entry?;
            let file_name = entry.file_name();

            if file_name.to_str().is_none_or(|hash| hash.starts_with('.')) {
                continue;
            }

            let url_link = std::path::Path::new(&file_name).join(".uri");

            tracing::debug!(hash = ?file_name, path=?url_link, "Reading `.uri` metadata file");
            let mut url = match self.root.read_to_string(&url_link) {
                Ok(url) => url,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            };
            // Also accept older metadata renamed without adding an LF.
            if url.ends_with('\n') {
                url.pop();
            }

            let current = self.current_version(&url)?;

            urls.push((url, current));
        }

        Ok(urls)
    }

    #[tracing::instrument(skip(self), fields(url = url.as_ref()))]
    fn list_snapshots(&self, url: impl AsRef<str>) -> Result<Vec<Timestamp>> {
        let url_hash = hex::encode(sha256(url.as_ref()));
        let url_dir = std::path::Path::new(&url_hash);

        let mut tss = Vec::new();

        tracing::debug!("Reading directory");
        let read_dir = self.root.read_dir(url_dir)?;
        for entry in read_dir {
            let entry = entry?;

            let filename = entry.file_name();
            let filename = std::path::Path::new(&filename);

            let stem = filename
                .file_stem()
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "Malformed file: `{}` does not have a valid file stem",
                        filename.display()
                    ))
                })?
                .to_string_lossy();

            if stem == "current" {
                continue;
            }
            if stem == ".uri" {
                continue;
            }

            let ts: Timestamp = stem
                .parse()
                .map_err(|e| std::io::Error::other(format!("Invalid timestamp `{stem}`: {e}")))?;

            tss.push(ts)
        }

        Ok(tss)
    }

    #[tracing::instrument(skip(self), fields(url = url.as_ref()))]
    fn delete(&self, url: impl AsRef<str>, timestamp: Timestamp) -> Result<()> {
        let url_hash = hex::encode(sha256(url.as_ref()));
        let url_dir = std::path::Path::new(&url_hash);

        let ts = timestamp.strftime(TIMESTAMP_FORMAT_STRING).to_string();
        let snapshot_dir_path = url_dir.join(ts);

        tracing::debug!(path = ?snapshot_dir_path, "Deleting snapshot");
        self.delete_dir(&snapshot_dir_path)?;

        let Ok(current) = self.current_version(&url) else {
            return Ok(());
        };

        if timestamp != current {
            return Ok(());
        }

        tracing::debug!("Deleted snapshot was `current`, searching for new `current`...");

        let versions = self.list_snapshots(&url)?;

        if let Some(new_current) = versions.into_iter().max() {
            tracing::debug!(?new_current, "Updating `current`");
            self.set_current_version(url, new_current)
        } else {
            tracing::debug!("Last snapshot deleted, removing `current`");
            self.delete_current_version(&url)
        }
    }
}

impl Fs {
    fn delete_current_version(&self, url: impl AsRef<str>) -> Result<()> {
        let url_hash = hex::encode(sha256(url.as_ref()));
        let url_dir = std::path::Path::new(&url_hash);

        let current_link_path = url_dir.join("current");

        self.delete_file(&current_link_path)
    }

    fn delete_file(&self, path: impl AsRef<Path>) -> Result<()> {
        // We call `std::fs::symlink_metadata` because the target file may
        // be a symlink and this does not follow the symlink like
        // `std::fs::metadata` does.
        #[cfg(windows)]
        match self.root.symlink_metadata(&path) {
            Ok(md) => {
                let mut permissions = md.permissions();
                if permissions.readonly() {
                    permissions.set_readonly(false);
                    self.root.set_permissions(&path, permissions)?;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        };

        self.root.remove_file(path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
    }

    fn delete_dir(&self, path: impl AsRef<Path>) -> Result<()> {
        // We call `std::fs::symlink_metadata` because the target file may
        // be a symlink and this does not follow the symlink like
        // `std::fs::metadata` does.
        #[cfg(windows)]
        match self.root.symlink_metadata(&path) {
            Ok(md) => {
                let mut permissions = md.permissions();
                if permissions.readonly() {
                    permissions.set_readonly(false);
                    self.root.set_permissions(&path, permissions)?;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        };

        let mut read_dir = self.root.read_dir(&path)?;
        while let Some(Ok(entry)) = read_dir.next() {
            self.delete_file(path.as_ref().join(entry.file_name()))?;
        }

        self.root.remove_dir(path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
    }
}

fn sha256(data: impl AsRef<[u8]>) -> sha2::digest::Output<sha2::Sha256> {
    use sha2::Digest;
    sha2::Sha256::digest(data.as_ref())
}

#[cfg(test)]
mod tests {
    use jiff::{ToSpan, Unit};

    use super::*;
    use crate::storage::Storage;
    use alloc::string::ToString;
    use std::eprintln;

    #[test]
    fn storage() {
        tracing_subscriber::fmt::init();

        let tmp_dir = tempfile::Builder::new()
            .prefix("asimov-snapshot-fs-test")
            .tempdir()
            .unwrap();
        let auth = cap_std::ambient_authority();
        cap_std::fs::Dir::create_ambient_dir_all(&tmp_dir.path(), auth).unwrap();
        let root = cap_std::fs::Dir::open_ambient_dir(&tmp_dir.path(), auth).unwrap();

        eprintln!("Testing directory: {tmp_dir:?}");

        let fs = Fs { root };

        let url = "HTTPS://例え.テスト/🦀/%2f?q=%41#片";
        let first_ts = Timestamp::now().round(Unit::Second).unwrap();

        let snapshot = Snapshot::builder()
            .url(url)
            .start_timestamp(first_ts)
            .end_timestamp(first_ts + 1.second())
            .data(r"v1")
            .build();
        fs.save(&snapshot).unwrap();

        let metadata_path = Path::new(&hex::encode(sha256(url))).join(".uri");
        assert_eq!(
            fs.root.read_to_string(&metadata_path).unwrap(),
            format!("{url}\n")
        );

        let current = fs.current_version(url).unwrap();
        assert_eq!(current, first_ts);

        let read_snapshot = fs.read_current(url).unwrap();
        assert_eq!(snapshot, read_snapshot);

        let second_ts = Timestamp::now()
            .round(Unit::Second)
            .unwrap()
            .checked_sub(1.hour())
            .unwrap();

        let snapshot = Snapshot::builder()
            .url(url)
            .start_timestamp(second_ts)
            .data(r"v2")
            .build();
        fs.save(&snapshot).unwrap();

        let current = fs.current_version(url).unwrap();
        assert_eq!(
            current, first_ts,
            "Saving older timestamps should not affect the `current` link"
        );

        let third_ts = Timestamp::now()
            .round(Unit::Second)
            .unwrap()
            .checked_add(1.hour())
            .unwrap();

        let snapshot = Snapshot::builder()
            .url(url)
            .start_timestamp(third_ts)
            .data(r"v3")
            .build();
        fs.save(&snapshot).unwrap();

        let current = fs.current_version(url).unwrap();
        assert_eq!(
            current, third_ts,
            "Saving newer timestamps should update the `current` link"
        );

        let urls = fs.list_urls().unwrap();
        assert_eq!(
            urls.as_slice(),
            &[(url.to_string(), third_ts)],
            "A single URL should be returned"
        );

        let snapshots = fs.list_snapshots(&url).unwrap();
        assert_eq!(snapshots.len(), 3);
        assert!(snapshots.contains(&first_ts));
        assert!(snapshots.contains(&second_ts));
        assert!(snapshots.contains(&third_ts));

        fs.delete(&url, third_ts).unwrap();
        assert_eq!(fs.list_snapshots(&url).unwrap().len(), 2);
        assert_eq!(fs.current_version(&url).unwrap(), first_ts);

        fs.delete(&url, first_ts).unwrap();
        assert_eq!(fs.current_version(&url).unwrap(), second_ts);
        assert_eq!(fs.list_snapshots(&url).unwrap().len(), 1);

        fs.delete(&url, second_ts).unwrap();
        assert_eq!(fs.list_snapshots(&url).unwrap().len(), 0);

        assert_eq!(
            fs.current_version(&url).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
