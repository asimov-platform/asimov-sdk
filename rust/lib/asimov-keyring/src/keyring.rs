// This is free and unencumbered software released into the public domain.

#![cfg(feature = "std")]

use super::KeyringError;
use alloc::string::ToString;
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
/// directory. User names are joined directly to this directory as file paths;
/// callers should supply names suitable for use as a single file name.
///
/// Opening and closing a keyring changes the process-wide default
/// [`keyring_core`] store, so multiple handles do not have independent store
/// lifetimes. Dropping a handle does not close the store; use [`Self::close`]
/// to unset it explicitly.
///
/// Available with the `std` feature.
pub struct Keyring {
    /// Directory containing the per-user, text-encoded public keys.
    path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static STORE_LOCK: Mutex<()> = Mutex::new(());

    fn with_keyring(test: impl FnOnce(&mut Keyring)) {
        // Serialize all default-store access; never open the OS keyring.
        let _lock = STORE_LOCK.lock().unwrap();
        struct MockStore;
        impl Drop for MockStore {
            fn drop(&mut self) {
                keyring_core::unset_default_store();
            }
        }
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let _store = MockStore;
        let directory = tempfile::tempdir().unwrap();
        let mut keyring = Keyring {
            path: directory.path().into(),
        };
        test(&mut keyring);
    }

    #[test]
    fn stored_secrets_require_exact_length() {
        with_keyring(|keyring| {
            let user = "secret-length-test";
            let entry = keyring_core::Entry::new(KEYRING_SERVICE, user).unwrap();
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

    fn store_secret(user: &str) -> SecretKey {
        let secret = Zeroizing::new([42; 32]);
        keyring_core::Entry::new(KEYRING_SERVICE, user)
            .unwrap()
            .set_secret(secret.as_slice())
            .unwrap();
        SecretKey::from_bytes(&secret)
    }

    #[test]
    fn missing_stale_and_malformed_caches_preserve_identity() {
        with_keyring(|keyring| {
            let user = "cache-test";
            let secret = store_secret(user);
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
            let secret = store_secret(user);
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
            let secret = store_secret(user);
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
            let secret = store_secret(user);
            let directory = keyring.path.clone();
            std::fs::write(directory.join("not-a-directory"), b"blocked").unwrap();
            for parent in ["missing-directory", "not-a-directory"] {
                keyring.path = directory.join(parent);
                assert!(matches!(
                    keyring.ensure_secret_key(user),
                    Err(KeyringError::IoError(_))
                ));
                assert!(matches!(keyring.rekey(user), Err(KeyringError::IoError(_))));
                assert_eq!(
                    keyring.get_secret_key(user).unwrap().unwrap().public(),
                    secret.public()
                );
            }

            keyring.path = directory;
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
            let entry = keyring_core::Entry::new(KEYRING_SERVICE, user).unwrap();
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
            let secret = store_secret(user);
            let entry = keyring_core::Entry::new(KEYRING_SERVICE, user).unwrap();
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
}

impl Keyring {
    /// Returns the current operating-system user's public key from their secret.
    ///
    /// Uses `"default"` as the user name if the operating-system user name cannot
    /// be obtained. Repairs the public-key cache from the existing secret, or
    /// creates a key pair only when no secret exists. Opens the process-wide
    /// store and closes it on both success and error.
    ///
    /// # Errors
    ///
    /// Returns an error if opening the keyring, retrieving the secret, repairing
    /// the cache, or storing a newly generated key pair fails.
    pub fn my_public_key() -> Result<PublicKey, KeyringError> {
        let mut keyring = Keyring::open()?;
        let user = whoami::username().unwrap_or_else(|_| "default".to_string());
        let public_key = keyring
            .ensure_secret_key(&user)
            .map(|secret| secret.public().into());
        keyring.close()?;
        public_key
    }

    /// Creates the public-key directory and installs the default keyring store.
    ///
    /// Uses the native Keychain store on Apple platforms, the native Windows
    /// store on Windows, and the kernel keyutils store on Linux. Other platforms
    /// use the mock store. This replaces the process-wide default store.
    ///
    /// # Errors
    ///
    /// Returns an error if locating or creating the state directory, or
    /// initializing the platform store, fails.
    pub fn open() -> Result<Self, KeyringError> {
        let path = StateDirectory::home()?.join("keyring");
        std::fs::create_dir_all(&path)?;

        // See: <https://docs.rs/apple-native-keyring-store/latest/apple_native_keyring_store/>
        #[cfg(target_vendor = "apple")]
        keyring_core::set_default_store(apple_native_keyring_store::keychain::Store::new()?);

        // See: <https://docs.rs/windows-native-keyring-store/latest/windows_native_keyring_store/>
        #[cfg(target_os = "windows")]
        keyring_core::set_default_store(windows_native_keyring_store::Store::new()?);

        // See: <https://docs.rs/linux-keyutils-keyring-store/latest/linux_keyutils_keyring_store/>
        #[cfg(target_os = "linux")]
        keyring_core::set_default_store(linux_keyutils_keyring_store::Store::new()?);

        // See: <https://docs.rs/keyring-core/latest/keyring_core/mock/index.html>
        #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_os = "linux")))]
        keyring_core::set_default_store(keyring_core::mock::Store::new()?);

        Ok(Self { path: path.into() })
    }

    /// Unsets the process-wide default keyring store without deleting any keys.
    ///
    /// This affects all handles using the default store, not just this handle.
    /// Currently always returns `Ok(())`.
    pub fn close(&self) -> Result<(), KeyringError> {
        keyring_core::unset_default_store();
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
        let key_path = self.path.join(user);
        match std::fs::read_to_string(&key_path) {
            Ok(encoded_pk) => Ok(Some(encoded_pk.trim().parse()?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Retrieves a user's secret key from the default keyring store.
    ///
    /// Returns `Ok(None)` when no entry exists for [`KEYRING_SERVICE`] and
    /// `user`. Temporary secret-byte buffers are zeroized on success and error.
    ///
    /// # Errors
    ///
    /// Returns an error if the keyring entry cannot be accessed or read, or
    /// [`KeyringError::CorruptSecret`] if it is not exactly 32 bytes long.
    pub fn get_secret_key(&self, user: &str) -> Result<Option<SecretKey>, KeyringError> {
        match keyring_core::Entry::new(KEYRING_SERVICE, &user)?.get_secret() {
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
        match self.get_secret_key(user)? {
            Some(secret_key) => {
                self.repair_public_key(user, secret_key.public().into())?;
                Ok(secret_key)
            },
            None => self.rekey(user).map(|(secret_key, _)| secret_key),
        }
    }

    /// Generates and stores a new key pair, replacing any existing keys for `user`.
    ///
    /// Returns the secret key followed by its public key. Stages and syncs the
    /// public-key file before changing the secret, then atomically publishes
    /// the cache. Callers must serialize key mutations for the same user.
    ///
    /// # Errors
    ///
    /// Returns an error if accessing storage or storing either key fails. If
    /// publishing the cache fails, restores the previous secret (or removes a
    /// newly created entry). If rollback also fails, returns
    /// [`KeyringError::RekeyRollbackFailed`]. After an interrupted rekey, use
    /// [`Self::ensure_secret_key`] to repair the cache from the stored secret.
    pub fn rekey(&mut self, user: &str) -> Result<(SecretKey, PublicKey), KeyringError> {
        let entry = keyring_core::Entry::new(KEYRING_SERVICE, user)?;
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
        if let Err(error) = staged.persist(self.path.join(user)) {
            let rollback = match previous_secret {
                Some(secret) => entry.set_secret(secret.as_slice()),
                None => entry.delete_credential(),
            };
            return Err(match rollback {
                Ok(()) => error.error.into(),
                Err(rollback_error) => KeyringError::RekeyRollbackFailed {
                    cache_error: error.error,
                    rollback_error,
                },
            });
        }
        Ok((secret_key, public_key))
    }

    fn repair_public_key(&self, user: &str, public_key: PublicKey) -> Result<(), KeyringError> {
        match self.get_public_key(user) {
            Ok(Some(cached)) if cached == public_key => return Ok(()),
            Ok(_) | Err(KeyringError::KeyError(_)) => {},
            Err(KeyringError::IoError(error))
                if error.kind() == std::io::ErrorKind::InvalidData => {},
            Err(error) => return Err(error),
        }
        self.stage_public_key(public_key)?
            .persist(self.path.join(user))
            .map_err(|error| error.error)?;
        Ok(())
    }

    fn stage_public_key(
        &self,
        public_key: PublicKey,
    ) -> Result<tempfile::NamedTempFile, std::io::Error> {
        let mut file = tempfile::NamedTempFile::new_in(&self.path)?;
        writeln!(file, "{public_key}")?;
        file.as_file().sync_all()?;
        Ok(file)
    }
}
