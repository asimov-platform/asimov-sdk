// This is free and unencumbered software released into the public domain.

#![cfg(feature = "std")]

use super::{KeyringError, store::Store};
use alloc::{string::ToString, sync::Arc};
use asimov_core::crates::cap_std::{self, fs::Dir};
use asimov_directory::fs::StateDirectory;
use asimov_id::PublicKey;
use iroh_base::SecretKey;
use secrecy::zeroize::Zeroizing;
use std::{io::Write, path::PathBuf};

/// Service name used to identify ASIMOV secret-key entries in the keyring store.
pub const KEYRING_SERVICE: &str = "sh.asimov";

/// Stores user secret keys in a platform keyring and public keys in local files.
///
/// Secret keys are indexed by [`KEYRING_SERVICE`] and a user name. Public keys
/// are stored as text in the `keyring` subdirectory of the ASIMOV home state
/// directory. User names must be portable, single filename components; invalid
/// names return [`KeyringError::InvalidUser`] before any backend or file access.
/// Reads cannot follow symlinks outside the opened cache directory, and atomic
/// writes replace the named entry rather than following its symlink target.
///
/// Handles own their backend through a shared guard. Closing or dropping one
/// handle leaves the others usable; the last handle releases the backend.
/// Operations for the same user and backend instance are serialized within
/// this process, including cache publication and rollback. Direct backend
/// access and other processes are outside this coordination.
/// Operations return [`KeyringError::LockPoisoned`] if a coordination lock
/// cannot be acquired because a previous holder panicked.
///
/// Available with the `std` feature.
#[derive(Clone)]
pub struct Keyring {
    // Retained only to locate fixtures; production I/O uses the directory handle.
    #[cfg(test)]
    path: PathBuf,
    directory: Arc<Dir>,
    store: Arc<Store>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_keyring(test: impl FnOnce(&mut Keyring)) {
        let directory = tempfile::tempdir().unwrap();
        let mut keyring =
            Keyring::with_store(directory.path(), keyring_core::mock::Store::new().unwrap())
                .unwrap();
        test(&mut keyring);
    }

    #[test]
    fn stored_secrets_require_exact_length() {
        with_keyring(|keyring| {
            let user = "secret-length-test";
            let entry = keyring.store.entry(user).unwrap();
            assert!(keyring.get_secret_key(user).unwrap().is_none());

            for length in [0, 1, 31, 33, 64] {
                let secret = Zeroizing::new(alloc::vec![42; length]);
                entry.set_secret(secret.as_slice()).unwrap();
                assert!(matches!(
                    keyring.get_secret_key(user),
                    Err(KeyringError::CorruptSecret { length: actual }) if actual == length
                ));
                assert!(matches!(
                    keyring.ensure_secret_key(user),
                    Err(KeyringError::CorruptSecret { length: actual }) if actual == length
                ));
                // Corruption must not be treated as absence and replaced by rekeying.
                let stored = Zeroizing::new(entry.get_secret().unwrap());
                assert_eq!(stored.as_slice(), secret.as_slice());
            }

            for byte in [0, 42, 255] {
                let secret = Zeroizing::new([byte; 32]);
                entry.set_secret(secret.as_slice()).unwrap();
                let key = keyring.get_secret_key(user).unwrap().unwrap();
                let actual = Zeroizing::new(key.to_bytes());
                assert_eq!(*actual, *secret);
                assert_eq!(
                    keyring.ensure_secret_key(user).unwrap().public(),
                    key.public()
                );
            }

            entry.delete_credential().unwrap();
            assert!(keyring.get_secret_key(user).unwrap().is_none());
        });
    }

    fn store_secret(keyring: &Keyring, user: &str) -> SecretKey {
        let secret = Zeroizing::new([42; 32]);
        keyring
            .store
            .entry(user)
            .unwrap()
            .set_secret(secret.as_slice())
            .unwrap();
        SecretKey::from_bytes(&secret)
    }

    #[test]
    fn missing_stale_and_malformed_caches_preserve_identity() {
        with_keyring(|keyring| {
            let user = "cache-test";
            let secret = store_secret(keyring, user);
            let expected: PublicKey = secret.public().into();
            let stale: PublicKey = SecretKey::from_bytes(&[7; 32]).public().into();
            let stale = stale.to_string();

            for cached in [
                None,
                Some(stale.as_bytes()),
                Some(b"bad key"),
                Some(&[255][..]),
            ] {
                let path = keyring.path.join(user);
                if let Some(bytes) = cached {
                    std::fs::write(&path, bytes).unwrap();
                }
                assert_eq!(
                    keyring.ensure_secret_key(user).unwrap().public(),
                    secret.public()
                );
                assert_eq!(
                    keyring.get_secret_key(user).unwrap().unwrap().public(),
                    secret.public()
                );
                assert_eq!(keyring.get_public_key(user).unwrap(), Some(expected));
                assert_eq!(
                    std::fs::read_to_string(path).unwrap(),
                    alloc::format!("{expected}\n")
                );
                assert_eq!(std::fs::read_dir(&keyring.path).unwrap().count(), 1);
            }
        });
    }

    #[test]
    fn matching_cache_is_not_rewritten() {
        with_keyring(|keyring| {
            let user = "matching-cache";
            let secret = store_secret(keyring, user);
            let expected: PublicKey = secret.public().into();
            let contents = alloc::format!("  {expected}\n\n");
            let path = keyring.path.join(user);
            std::fs::write(&path, &contents).unwrap();
            keyring.ensure_secret_key(user).unwrap();
            assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
        });
    }

    #[test]
    fn stale_cache_is_replaced_atomically() {
        with_keyring(|keyring| {
            use std::io::Read;

            let user = "atomic-cache";
            let secret = store_secret(keyring, user);
            let expected: PublicKey = secret.public().into();
            let path = keyring.path.join(user);
            std::fs::write(&path, "stale cache").unwrap();
            let mut old_file = std::fs::File::open(&path).unwrap();
            keyring.ensure_secret_key(user).unwrap();
            let mut old_contents = alloc::string::String::new();
            old_file.read_to_string(&mut old_contents).unwrap();
            assert_eq!(old_contents, "stale cache");
            assert_eq!(keyring.get_public_key(user).unwrap(), Some(expected));
        });
    }

    #[test]
    fn unwritable_cache_preserves_secret_and_can_be_retried() {
        with_keyring(|keyring| {
            let user = "blocked-cache";
            let secret = store_secret(keyring, user);
            let blocked = keyring.path.join(user);
            std::fs::create_dir(&blocked).unwrap();
            assert!(matches!(
                keyring.ensure_secret_key(user),
                Err(KeyringError::IoError(_))
            ));
            assert!(matches!(keyring.rekey(user), Err(KeyringError::IoError(_))));
            assert_eq!(
                keyring.get_secret_key(user).unwrap().unwrap().public(),
                secret.public()
            );
            std::fs::remove_dir(blocked).unwrap();
            assert_eq!(
                keyring.ensure_secret_key(user).unwrap().public(),
                secret.public()
            );
            assert_eq!(
                keyring.get_public_key(user).unwrap(),
                Some(secret.public().into())
            );
        });
    }

    #[test]
    fn failed_rekey_publication_restores_the_previous_entry() {
        with_keyring(|keyring| {
            let user = "failed-rekey";
            let entry = keyring.store.entry(user).unwrap();
            let path = keyring.path.join(user);
            // A directory at the destination allows staging but prevents rename.
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("sentinel"), b"untouched").unwrap();

            for length in [None, Some(32), Some(3)] {
                let previous = length.map(|length| Zeroizing::new(alloc::vec![42; length]));
                if let Some(secret) = &previous {
                    entry.set_secret(secret.as_slice()).unwrap();
                }
                assert!(matches!(keyring.rekey(user), Err(KeyringError::IoError(_))));
                match previous {
                    Some(secret) => {
                        let stored = Zeroizing::new(entry.get_secret().unwrap());
                        assert_eq!(stored.as_slice(), secret.as_slice());
                        entry.delete_credential().unwrap();
                    },
                    None => assert!(matches!(
                        entry.get_secret(),
                        Err(keyring_core::Error::NoEntry)
                    )),
                }
                assert_eq!(std::fs::read(path.join("sentinel")).unwrap(), b"untouched");
                assert_eq!(std::fs::read_dir(&keyring.path).unwrap().count(), 1);
            }

            std::fs::remove_dir_all(path).unwrap();
            let (secret, public) = keyring.rekey(user).unwrap();
            assert_eq!(PublicKey::from(secret.public()), public);
            assert_eq!(
                keyring.get_secret_key(user).unwrap().unwrap().public(),
                secret.public()
            );
            assert_eq!(keyring.get_public_key(user).unwrap(), Some(public));
        });
    }

    #[test]
    fn absent_secret_is_created_and_explicit_rekey_rotates_it() {
        with_keyring(|keyring| {
            let user = "new-key";
            let first = keyring.ensure_secret_key(user).unwrap();
            assert_eq!(
                keyring.get_public_key(user).unwrap(),
                Some(first.public().into())
            );
            let (second, public) = keyring.rekey(user).unwrap();
            assert_ne!(second.public(), first.public());
            assert_eq!(keyring.get_public_key(user).unwrap(), Some(public));
            assert_eq!(
                keyring.get_secret_key(user).unwrap().unwrap().public(),
                second.public()
            );
        });
    }

    #[test]
    fn backend_read_errors_do_not_rotate_or_publish() {
        with_keyring(|keyring| {
            let user = "backend-error";
            let secret = store_secret(keyring, user);
            let entry = keyring.store.entry(user).unwrap();
            let mock: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
            let path = keyring.path.join(user);
            std::fs::write(&path, "untouched cache").unwrap();

            for rekey in [false, true] {
                mock.set_error(keyring_core::Error::Invalid(
                    "injected".into(),
                    "read failure".into(),
                ));
                let result = if rekey {
                    keyring.rekey(user).map(|_| ())
                } else {
                    keyring.ensure_secret_key(user).map(|_| ())
                };
                assert!(matches!(result, Err(KeyringError::KeyringError(_))));
                assert_eq!(
                    keyring.get_secret_key(user).unwrap().unwrap().public(),
                    secret.public()
                );
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "untouched cache");
            }
        });
    }

    #[test]
    fn overlapping_handles_release_only_their_own_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let backend = keyring_core::mock::Store::new().unwrap();
        let weak = Arc::downgrade(&backend);
        let mut first = Keyring::with_store(directory.path(), backend.clone()).unwrap();
        let second = Keyring::with_store(directory.path(), backend).unwrap();
        assert!(Arc::ptr_eq(&first.store, &second.store));
        let secret = first.ensure_secret_key("shared").unwrap();
        let third = first.clone();

        first.close().unwrap();
        assert_eq!(
            second.get_secret_key("shared").unwrap().unwrap().public(),
            secret.public()
        );
        drop(second);
        assert!(weak.upgrade().is_some());
        assert_eq!(
            third.get_public_key("shared").unwrap(),
            Some(secret.public().into())
        );
        third.close().unwrap();
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn separate_backends_are_independent() {
        with_keyring(|first| {
            with_keyring(|second| {
                let first_key = first.ensure_secret_key("same-user").unwrap();
                assert!(second.get_secret_key("same-user").unwrap().is_none());
                let second_key = second.ensure_secret_key("same-user").unwrap();
                assert_ne!(first_key.public(), second_key.public());
                first.rekey("same-user").unwrap();
                assert_eq!(
                    second
                        .get_secret_key("same-user")
                        .unwrap()
                        .unwrap()
                        .public(),
                    second_key.public()
                );
            });
        });
    }

    #[test]
    fn errors_release_handles_and_operation_locks() {
        let directory = tempfile::tempdir().unwrap();
        let backend = keyring_core::mock::Store::new().unwrap();
        let weak = Arc::downgrade(&backend);
        let mut survivor = Keyring::with_store(directory.path(), backend.clone()).unwrap();
        let secret = survivor.ensure_secret_key("error-test").unwrap();
        let blocked_cache = directory.path().join("error-test");
        std::fs::remove_file(&blocked_cache).unwrap();
        std::fs::create_dir(&blocked_cache).unwrap();
        let failed = || -> Result<(), KeyringError> {
            let mut handle = Keyring::with_store(directory.path(), backend.clone())?;
            handle.ensure_secret_key("error-test")?;
            Ok(())
        };
        assert!(matches!(failed(), Err(KeyringError::IoError(_))));
        std::fs::remove_dir(blocked_cache).unwrap();
        assert_eq!(
            survivor.ensure_secret_key("error-test").unwrap().public(),
            secret.public()
        );
        drop(survivor);

        // Failed construction must not retain the caller's backend either.
        let blocked = directory.path().join("file");
        std::fs::write(&blocked, b"not a directory").unwrap();
        assert!(Keyring::with_store(blocked, backend).is_err());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn handles_do_not_change_the_process_default_store() {
        struct RestoreDefault(Option<Arc<keyring_core::CredentialStore>>);
        impl Drop for RestoreDefault {
            fn drop(&mut self) {
                if let Some(previous) = self.0.take() {
                    keyring_core::set_default_store(previous);
                } else {
                    keyring_core::unset_default_store();
                }
            }
        }
        // Only this test accesses the global default; all fixtures are injected.
        let _restore = RestoreDefault(keyring_core::get_default_store());
        let default: Arc<keyring_core::CredentialStore> = keyring_core::mock::Store::new().unwrap();
        keyring_core::set_default_store(default.clone());
        with_keyring(|handle| {
            handle.ensure_secret_key("private").unwrap();
            assert!(matches!(
                keyring_core::Entry::new(KEYRING_SERVICE, "private")
                    .unwrap()
                    .get_secret(),
                Err(keyring_core::Error::NoEntry)
            ));
            handle.clone().close().unwrap();
            assert!(Arc::ptr_eq(
                &keyring_core::get_default_store().unwrap(),
                &default
            ));
        });
        assert!(Arc::ptr_eq(
            &keyring_core::get_default_store().unwrap(),
            &default
        ));
    }

    #[test]
    fn concurrent_ensure_creates_one_identity() {
        let directory = tempfile::tempdir().unwrap();
        let backend = keyring_core::mock::Store::new().unwrap();
        let observer = Keyring::with_store(directory.path(), backend.clone()).unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let tasks: alloc::vec::Vec<_> = (0..8)
                .map(|_| {
                    let mut handle =
                        Keyring::with_store(directory.path(), backend.clone()).unwrap();
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        handle.ensure_secret_key("concurrent").unwrap().public()
                    })
                })
                .collect();
            let keys: alloc::vec::Vec<_> =
                tasks.into_iter().map(|task| task.join().unwrap()).collect();
            assert!(keys.iter().all(|key| *key == keys[0]));
            assert_eq!(
                observer.get_public_key("concurrent").unwrap(),
                Some(keys[0].into())
            );
        });
    }

    #[test]
    fn concurrent_rekey_and_repair_leave_a_consistent_pair() {
        with_keyring(|observer| {
            observer.ensure_secret_key("rotating").unwrap();
            let barrier = std::sync::Barrier::new(4);
            std::thread::scope(|scope| {
                for index in 0..4 {
                    let mut handle = observer.clone();
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        for _ in 0..16 {
                            if index % 2 == 0 {
                                handle.rekey("rotating").unwrap();
                            } else {
                                handle.ensure_secret_key("rotating").unwrap();
                            }
                        }
                    });
                }
            });
            let secret = observer.get_secret_key("rotating").unwrap().unwrap();
            assert_eq!(
                observer.get_public_key("rotating").unwrap(),
                Some(secret.public().into())
            );
        });
    }

    #[test]
    fn rejects_invalid_users_before_accessing_storage() {
        with_keyring(|keyring| {
            for user in [
                "",
                ".",
                "..",
                "../victim",
                "/tmp/victim",
                "a/b",
                "a\\b",
                "C:\\victim",
                "C:victim",
                "\\\\server\\share",
                "key:stream",
                "key\0",
                "key.",
                "key ",
                "NUL",
                "con.txt",
            ] {
                assert!(
                    matches!(
                        keyring.get_public_key(user),
                        Err(KeyringError::InvalidUser(_))
                    ),
                    "{user:?}"
                );
                assert!(
                    matches!(
                        keyring.get_secret_key(user),
                        Err(KeyringError::InvalidUser(_))
                    ),
                    "{user:?}"
                );
                assert!(
                    matches!(
                        keyring.ensure_secret_key(user),
                        Err(KeyringError::InvalidUser(_))
                    ),
                    "{user:?}"
                );
                assert!(
                    matches!(keyring.rekey(user), Err(KeyringError::InvalidUser(_))),
                    "{user:?}"
                );
                assert!(matches!(
                    keyring.store.entry(user).unwrap().get_secret(),
                    Err(keyring_core::Error::NoEntry)
                ));
            }
            assert_eq!(std::fs::read_dir(&keyring.path).unwrap().count(), 0);
            for user in ["alice@example.org", "Alice Smith", "用户"] {
                let secret = keyring.ensure_secret_key(user).unwrap();
                assert_eq!(
                    keyring.get_public_key(user).unwrap(),
                    Some(secret.public().into())
                );
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn escaping_cache_symlinks_are_not_read_or_written_through() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, "untouched").unwrap();
        let mut keyring = Keyring::with_store(
            root.path().join("keys"),
            keyring_core::mock::Store::new().unwrap(),
        )
        .unwrap();
        let user = "alice";
        let secret = store_secret(&keyring, user);
        for target in [
            outside,
            root.path().join("missing"),
            PathBuf::from("../outside"),
        ] {
            let cache = keyring.path.join(user);
            symlink(target, &cache).unwrap();
            assert!(matches!(
                keyring.get_public_key(user),
                Err(KeyringError::IoError(_))
            ));
            assert!(matches!(
                keyring.ensure_secret_key(user),
                Err(KeyringError::IoError(_))
            ));
            assert_eq!(
                keyring.get_secret_key(user).unwrap().unwrap().public(),
                secret.public()
            );
            std::fs::remove_file(&cache).unwrap();
        }
        // Atomic publication replaces the link itself, never its target.
        symlink("../outside", keyring.path.join(user)).unwrap();
        let (_, public) = keyring.rekey(user).unwrap();
        assert_eq!(keyring.get_public_key(user).unwrap(), Some(public));
        assert!(
            !std::fs::symlink_metadata(keyring.path.join(user))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("outside")).unwrap(),
            "untouched"
        );
        assert!(!root.path().join("missing").exists());
        symlink(user, keyring.path.join("alias")).unwrap();
        assert_eq!(keyring.get_public_key("alias").unwrap(), Some(public));
    }

    #[cfg(unix)]
    #[test]
    fn replacing_the_cache_path_does_not_redirect_an_open_handle() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("keys");
        let moved = root.path().join("moved");
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("alice"), "untouched").unwrap();
        let mut keyring =
            Keyring::with_store(&cache, keyring_core::mock::Store::new().unwrap()).unwrap();
        std::fs::rename(&cache, &moved).unwrap();
        std::os::unix::fs::symlink(&outside, &cache).unwrap();
        let (_, public) = keyring.rekey("alice").unwrap();
        assert_eq!(keyring.get_public_key("alice").unwrap(), Some(public));
        assert_eq!(
            std::fs::read_to_string(moved.join("alice")).unwrap(),
            alloc::format!("{public}\n")
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("alice")).unwrap(),
            "untouched"
        );
    }
}

impl Keyring {
    /// Returns the current operating-system user's public key from their secret.
    ///
    /// Uses `"default"` as the user name if the operating-system user name cannot
    /// be obtained. Repairs the public-key cache from the existing secret, or
    /// creates a key pair only when no secret exists. Releases its backend
    /// handle on both success and error.
    ///
    /// # Errors
    ///
    /// Returns an error if opening the keyring, retrieving the secret, repairing
    /// the cache, or storing a newly generated key pair fails.
    pub fn my_public_key() -> Result<PublicKey, KeyringError> {
        let mut keyring = Keyring::open()?;
        let user = whoami::username().unwrap_or_else(|_| "default".to_string());
        keyring
            .ensure_secret_key(&user)
            .map(|secret| secret.public().into())
    }

    /// Creates the public-key directory and shares the platform keyring store.
    ///
    /// Uses the native Keychain store on Apple platforms, the native Windows
    /// store on Windows, and the kernel keyutils store on Linux. Other platforms
    /// use the mock store. Overlapping handles reuse the same backend. Does
    /// not read or change the process-wide `keyring_core` default store.
    ///
    /// # Errors
    ///
    /// Returns an error if locating or creating the state directory, or
    /// initializing the platform store, fails.
    pub fn open() -> Result<Self, KeyringError> {
        let state = StateDirectory::home()?;
        let root = Dir::open_ambient_dir(state.as_str(), cap_std::ambient_authority())?;
        root.create_dir_all("keyring")?;
        let directory = Arc::new(root.open_dir("keyring")?);
        Ok(Self {
            #[cfg(test)]
            path: state.join("keyring").into(),
            directory,
            store: Store::platform()?,
        })
    }

    /// Opens a handle with an explicit backend and public-key cache directory.
    ///
    /// Handles supplied clones of the same backend [`Arc`] share coordination.
    /// Distinct backend instances are independent. Creates the directory if
    /// needed, without changing the `keyring_core` default store. The backend
    /// remains alive while any handle or caller retains ownership.
    /// The supplied path selects the trusted cache root. Its opened directory
    /// handle is retained, so later path/symlink replacement cannot redirect I/O.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created or a
    /// coordination lock has been poisoned.
    pub fn with_store(
        path: impl Into<PathBuf>,
        backend: Arc<keyring_core::CredentialStore>,
    ) -> Result<Self, KeyringError> {
        let path = path.into();
        std::fs::create_dir_all(&path)?;
        let directory = Arc::new(Dir::open_ambient_dir(&path, cap_std::ambient_authority())?);
        Ok(Self {
            #[cfg(test)]
            path,
            directory,
            store: Store::shared(backend)?,
        })
    }

    /// Consumes this handle without deleting keys or affecting other handles.
    ///
    /// Equivalent to dropping it. Currently always returns `Ok(())`.
    pub fn close(self) -> Result<(), KeyringError> {
        drop(self);
        Ok(())
    }

    /// Reads a user's public key from its local file.
    ///
    /// Trims surrounding whitespace before parsing. Returns `Ok(None)` when
    /// the file does not exist; does not consult the secret-key store.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read for a reason other than a
    /// missing file, or its contents cannot be parsed as a public key.
    pub fn get_public_key(&self, user: &str) -> Result<Option<PublicKey>, KeyringError> {
        let lock = self.store.user_lock(user)?;
        let _guard = lock.lock().map_err(|_| KeyringError::LockPoisoned)?;
        self.read_public_key(user)
    }

    // Internal helpers run while the public operation holds the user's lock.
    fn read_public_key(&self, user: &str) -> Result<Option<PublicKey>, KeyringError> {
        match self.directory.read_to_string(user) {
            Ok(encoded_pk) => Ok(Some(encoded_pk.trim().parse()?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Retrieves a user's secret key from this handle's backend.
    ///
    /// Returns `Ok(None)` when no entry exists for [`KEYRING_SERVICE`] and
    /// `user`. Temporary secret-byte buffers are zeroized on success and error.
    ///
    /// # Errors
    ///
    /// Returns an error if the keyring entry cannot be accessed or read, or
    /// [`KeyringError::CorruptSecret`] if it is not exactly 32 bytes long.
    pub fn get_secret_key(&self, user: &str) -> Result<Option<SecretKey>, KeyringError> {
        let lock = self.store.user_lock(user)?;
        let _guard = lock.lock().map_err(|_| KeyringError::LockPoisoned)?;
        self.read_secret_key(user)
    }

    fn read_secret_key(&self, user: &str) -> Result<Option<SecretKey>, KeyringError> {
        match self.store.entry(user)?.get_secret() {
            Ok(secret) => {
                let secret = Zeroizing::new(secret);
                let secret_bytes: &[u8; 32] =
                    secret
                        .as_slice()
                        .try_into()
                        .map_err(|_| KeyringError::CorruptSecret {
                            length: secret.len(),
                        })?;
                Ok(Some(SecretKey::from_bytes(secret_bytes)))
            },
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns a user's existing secret key, or generates and stores a key pair.
    ///
    /// Atomically repairs a missing, stale, or malformed public-key cache from
    /// the stored secret. A matching cache is left untouched. If no secret
    /// exists, delegates to [`Self::rekey`].
    ///
    /// # Errors
    ///
    /// Returns an error if retrieving the secret, repairing the cache, or
    /// storing a new key pair fails, including [`KeyringError::CorruptSecret`]
    /// for a stored secret that is not exactly 32 bytes long. A cache repair
    /// failure never changes the stored secret; retry after fixing the cache.
    pub fn ensure_secret_key(&mut self, user: &str) -> Result<SecretKey, KeyringError> {
        let lock = self.store.user_lock(user)?;
        let _guard = lock.lock().map_err(|_| KeyringError::LockPoisoned)?;
        match self.read_secret_key(user)? {
            Some(secret_key) => {
                self.repair_public_key(user, secret_key.public().into())?;
                Ok(secret_key)
            },
            None => self
                .replace_key_pair(user)
                .map(|(secret_key, _)| secret_key),
        }
    }

    /// Generates and stores a new key pair, replacing any existing keys for `user`.
    ///
    /// Returns the secret key followed by its public key. Stages and syncs the
    /// public-key file before changing the secret, then atomically publishes
    /// the cache. Holds the shared user's lock through publication or rollback.
    ///
    /// # Errors
    ///
    /// Returns an error if accessing storage or storing either key fails. If
    /// publishing the cache fails, restores the previous secret (or removes a
    /// newly created entry). If rollback also fails, returns
    /// [`KeyringError::RekeyRollbackFailed`]. After an interrupted rekey, use
    /// [`Self::ensure_secret_key`] to repair the cache from the stored secret.
    pub fn rekey(&mut self, user: &str) -> Result<(SecretKey, PublicKey), KeyringError> {
        let lock = self.store.user_lock(user)?;
        let _guard = lock.lock().map_err(|_| KeyringError::LockPoisoned)?;
        self.replace_key_pair(user)
    }

    fn replace_key_pair(&self, user: &str) -> Result<(SecretKey, PublicKey), KeyringError> {
        let entry = self.store.entry(user)?;
        let previous_secret = match entry.get_secret() {
            Ok(secret) => Some(Zeroizing::new(secret)),
            Err(keyring_core::Error::NoEntry) => None,
            Err(error) => return Err(error.into()),
        };
        let secret_key = SecretKey::generate();
        let public_key = secret_key.public().into();
        let staged = self.stage_public_key(public_key)?;
        {
            let secret_bytes = Zeroizing::new(secret_key.to_bytes());
            entry.set_secret(secret_bytes.as_slice())?;
        }
        if let Err(error) = staged.replace(user) {
            let rollback = match previous_secret {
                Some(secret) => entry.set_secret(secret.as_slice()),
                None => entry.delete_credential(),
            };
            return Err(match rollback {
                Ok(()) => error.into(),
                Err(rollback_error) => KeyringError::RekeyRollbackFailed {
                    cache_error: error,
                    rollback_error,
                },
            });
        }
        Ok((secret_key, public_key))
    }

    fn repair_public_key(&self, user: &str, public_key: PublicKey) -> Result<(), KeyringError> {
        match self.read_public_key(user) {
            Ok(Some(cached)) if cached == public_key => return Ok(()),
            Ok(_) | Err(KeyringError::KeyError(_)) => {},
            Err(KeyringError::IoError(error))
                if error.kind() == std::io::ErrorKind::InvalidData => {},
            Err(error) => return Err(error),
        }
        self.stage_public_key(public_key)?.replace(user)?;
        Ok(())
    }

    fn stage_public_key(
        &self,
        public_key: PublicKey,
    ) -> Result<cap_tempfile::TempFile<'_>, std::io::Error> {
        let mut file = cap_tempfile::TempFile::new(&self.directory)?;
        #[cfg(unix)]
        {
            use cap_std::fs::{Permissions, PermissionsExt};
            file.as_file()
                .set_permissions(Permissions::from_mode(0o600))?;
        }
        writeln!(file, "{public_key}")?;
        file.as_file().sync_all()?;
        Ok(file)
    }
}
