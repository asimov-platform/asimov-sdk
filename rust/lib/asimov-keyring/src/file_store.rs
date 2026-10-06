// This is free and unencumbered software released into the public domain.

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use asimov_core::crates::cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt, File, Metadata, MetadataExt, OpenOptions, OpenOptionsExt,
    Permissions, PermissionsExt,
};
use core::any::Any;
use keyring_core::{
    Credential, Entry, Error, Result,
    api::{CredentialApi, CredentialStoreApi},
};
use secrecy::zeroize::Zeroizing;
use std::{
    collections::HashMap,
    io::{self, Read, Write},
};

pub(crate) const DIRECTORY: &str = ".keyring";

/// Private to this crate: only ASIMOV identity entries use this file layout.
pub(crate) struct FileStore {
    directory: Arc<Dir>,
}

impl FileStore {
    pub(crate) fn new(root: &Dir) -> io::Result<Self> {
        Ok(Self {
            directory: Arc::new(private_directory(root, DIRECTORY)?),
        })
    }

    fn user_directory(&self, user: &str) -> io::Result<Dir> {
        asimov_core::validate_filename_component(user)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        private_directory(&self.directory, user)
    }

    pub(crate) fn lock(&self, user: &str) -> io::Result<std::fs::File> {
        let directory = self.user_directory(user)?;
        // Never replace or delete the lock file: every process must lock the
        // same inode, including while an identity is being created or rekeyed.
        // Separate creation from opening to handle concurrent first use on macOS.
        reject_symlink(&directory, "lock")?;
        let file = match directory.open_with(
            "lock",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600),
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                directory.open_with("lock", OpenOptions::new().read(true).write(true))?
            },
            Err(error) => return Err(error),
        };
        check_private_file(&file.metadata()?)?;
        let file = file.into_std();
        file.lock()?;
        Ok(file)
    }
}

impl CredentialStoreApi for FileStore {
    fn vendor(&self) -> String {
        "asimov-keyring/file".into()
    }

    fn id(&self) -> String {
        format!("{:p}", Arc::as_ptr(&self.directory))
    }

    fn build(
        &self,
        service: &str,
        user: &str,
        modifiers: Option<&HashMap<&str, &str>>,
    ) -> Result<Entry> {
        if service != crate::KEYRING_SERVICE || modifiers.is_some_and(|m| !m.is_empty()) {
            return Err(Error::NotSupportedByStore(
                "only ASIMOV identity entries are supported".into(),
            ));
        }
        let directory = Arc::new(self.user_directory(user).map_err(storage_error)?);
        Ok(Entry::new_with_credential(Arc::new(FileCredential {
            directory,
            user: user.into(),
        })))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
struct FileCredential {
    directory: Arc<Dir>,
    user: String,
}

impl FileCredential {
    fn open(&self) -> Result<File> {
        reject_symlink(&self.directory, "secret").map_err(storage_error)?;
        let file = self.directory.open("secret").map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Error::NoEntry
            } else {
                storage_error(error)
            }
        })?;
        check_private_file(&file.metadata().map_err(storage_error)?).map_err(storage_error)?;
        Ok(file)
    }
}

impl CredentialApi for FileCredential {
    fn get_secret(&self) -> Result<Vec<u8>> {
        let mut file = self.open()?;
        let mut secret = Zeroizing::new(Vec::new());
        file.read_to_end(&mut secret).map_err(storage_error)?;
        // The keyring caller owns and zeroizes the returned buffer. On read
        // failure, the partially filled buffer is zeroized here.
        Ok(core::mem::take(&mut *secret))
    }

    fn set_secret(&self, secret: &[u8]) -> Result<()> {
        let write = || -> io::Result<()> {
            let mut staged = cap_tempfile::TempFile::new(&self.directory)?;
            staged
                .as_file()
                .set_permissions(Permissions::from_mode(0o600))?;
            staged.write_all(secret)?;
            staged.as_file().sync_all()?;
            staged.replace("secret")?;
            sync_directory(&self.directory)
        };
        write().map_err(storage_error)
    }

    fn delete_credential(&self) -> Result<()> {
        self.open()?;
        self.directory
            .remove_file("secret")
            .map_err(storage_error)?;
        sync_directory(&self.directory).map_err(storage_error)
    }

    fn get_attributes(&self) -> Result<HashMap<String, String>> {
        self.open()?;
        Ok(HashMap::new())
    }

    fn get_credential(&self) -> Result<Option<Arc<Credential>>> {
        self.open()?;
        Ok(Some(Arc::new(self.clone())))
    }

    fn get_specifiers(&self) -> Option<(String, String)> {
        Some((crate::KEYRING_SERVICE.to_string(), self.user.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn private_directory(parent: &Dir, name: &str) -> io::Result<Dir> {
    match parent.create_dir_with(name, DirBuilder::new().mode(0o700)) {
        Ok(()) => sync_directory(parent)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error),
    }
    reject_symlink(parent, name)?;
    let directory = parent.open_dir(name)?;
    check_permissions(&directory.dir_metadata()?)?;
    Ok(directory)
}

fn reject_symlink(directory: &Dir, name: &str) -> io::Result<()> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file keyring does not accept symlinks",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn check_permissions(metadata: &Metadata) -> io::Result<()> {
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file keyring requires owner-only permissions (0700 directories, 0600 files)",
        ));
    }
    Ok(())
}

fn check_private_file(metadata: &Metadata) -> io::Result<()> {
    check_permissions(metadata)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file keyring requires regular files without additional hard links",
        ));
    }
    Ok(())
}

fn storage_error(error: io::Error) -> Error {
    Error::NoStorageAccess(Box::new(error))
}

fn sync_directory(directory: &Dir) -> io::Result<()> {
    // On Linux a Dir may hold an O_PATH descriptor, which cannot be fsynced.
    // Open a readable descriptor relative to the retained directory handle.
    directory.open(".")?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Keyring, KeyringError};
    use asimov_core::crates::cap_std;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt as _, symlink},
        path::Path,
    };

    fn keyring(root: &Path) -> Keyring {
        let directory = Dir::open_ambient_dir(root, cap_std::ambient_authority()).unwrap();
        Keyring::with_store(
            root.join("public"),
            Arc::new(FileStore::new(&directory).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn private_storage_survives_reopen_and_replaces_secrets_atomically() {
        let root = tempfile::tempdir().unwrap();
        let mut first = keyring(root.path());
        let key = first.ensure_secret_key("alice").unwrap().public();
        drop(first);
        let mut second = keyring(root.path());
        assert_eq!(second.ensure_secret_key("alice").unwrap().public(), key);
        let directory = root.path().join(".keyring/alice");
        for (path, mode) in [
            (root.path().join(".keyring"), 0o700),
            (directory.clone(), 0o700),
            (directory.join("secret"), 0o600),
            (directory.join("lock"), 0o600),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
        fs::write(root.path().join("public/alice"), "broken cache").unwrap();
        assert_eq!(second.ensure_secret_key("alice").unwrap().public(), key);
        assert_eq!(second.get_public_key("alice").unwrap(), Some(key.into()));

        let mut old_file = fs::File::open(directory.join("secret")).unwrap();
        let replacement = second.rekey("alice").unwrap().0.public();
        assert_ne!(key, replacement);
        let mut old_secret = Zeroizing::new([0; 32]);
        old_file.read_exact(&mut *old_secret).unwrap();
        assert_eq!(iroh_base::SecretKey::from_bytes(&old_secret).public(), key);
        assert_eq!(
            keyring(root.path())
                .ensure_secret_key("alice")
                .unwrap()
                .public(),
            replacement
        );
    }

    #[test]
    fn errors_and_failed_publication_preserve_existing_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut store = keyring(root.path());
        let key = store.ensure_secret_key("alice").unwrap().public();
        let secret = root.path().join(".keyring/alice/secret");
        let cache = root.path().join("public/alice");
        let original = Zeroizing::new(fs::read(&secret).unwrap());
        let cached = fs::read(&cache).unwrap();
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.ensure_secret_key("alice").is_err());
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            Zeroizing::new(fs::read(&secret).unwrap()).as_slice(),
            original.as_slice()
        );
        assert_eq!(fs::read(&cache).unwrap(), cached);

        for length in [0, 3, 31, 33, 64] {
            let corrupt = Zeroizing::new(alloc::vec![42; length]);
            fs::write(&secret, &*corrupt).unwrap();
            assert!(matches!(store.ensure_secret_key("alice"),
                Err(KeyringError::CorruptSecret { length: actual }) if actual == length));
            assert_eq!(
                Zeroizing::new(fs::read(&secret).unwrap()).as_slice(),
                corrupt.as_slice()
            );
            assert_eq!(fs::read(&cache).unwrap(), cached);
        }
        fs::write(&secret, &*original).unwrap();
        fs::remove_file(&cache).unwrap();
        fs::create_dir(&cache).unwrap();
        assert!(matches!(
            store.rekey("alice"),
            Err(KeyringError::IoError(_))
        ));
        assert_eq!(
            store.get_secret_key("alice").unwrap().unwrap().public(),
            key
        );
        fs::remove_dir(cache).unwrap();
        assert_eq!(store.ensure_secret_key("alice").unwrap().public(), key);

        fs::create_dir(root.path().join("public/new-user")).unwrap();
        assert!(store.ensure_secret_key("new-user").is_err());
        assert!(store.get_secret_key("new-user").unwrap().is_none());
    }

    #[test]
    fn unsafe_paths_and_permissive_directories_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let mut store = keyring(root.path());
        for user in ["../outside", "a/b", "a\\b", "", ".", ".."] {
            assert!(matches!(
                store.ensure_secret_key(user),
                Err(KeyringError::InvalidUser(_))
            ));
        }
        store.ensure_secret_key("alice").unwrap();
        let directory = root.path().join(".keyring/alice");
        let outside = root.path().join("outside");
        fs::write(&outside, [42; 32]).unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        for name in ["secret", "lock"] {
            let path = directory.join(name);
            let original = directory.join(format!("{name}.original"));
            fs::rename(&path, &original).unwrap();
            symlink(&outside, &path).unwrap();
            assert!(store.ensure_secret_key("alice").is_err());
            fs::remove_file(&path).unwrap();
            fs::hard_link(&outside, &path).unwrap();
            assert!(store.ensure_secret_key("alice").is_err());
            fs::remove_file(&path).unwrap();
            fs::rename(original, path).unwrap();
        }
        assert_eq!(fs::read(&outside).unwrap(), [42; 32]);
        symlink(&directory, root.path().join(".keyring/bob")).unwrap();
        assert!(store.ensure_secret_key("bob").is_err());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(store.ensure_secret_key("alice").is_err());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(store.ensure_secret_key("alice").is_ok());
    }
}
