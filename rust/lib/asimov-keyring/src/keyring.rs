// This is free and unencumbered software released into the public domain.

#![cfg(feature = "std")]

use super::KeyringError;
use alloc::string::ToString;
use asimov_directory::fs::StateDirectory;
use asimov_id::PublicKey;
use iroh_base::SecretKey;
use secrecy::zeroize::{Zeroize, Zeroizing};
use std::path::PathBuf;

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

impl Keyring {
    /// Returns the current operating-system user's public key, creating one if absent.
    ///
    /// Uses `"default"` as the user name if the operating-system user name cannot
    /// be obtained. If the public-key file is missing, generates a new key pair
    /// even if a secret key already exists. Opens the process-wide store and
    /// closes it on success; an earlier error leaves the store configured.
    ///
    /// # Errors
    ///
    /// Returns an error if opening the keyring, reading or parsing the public
    /// key, or storing a newly generated key pair fails.
    pub fn my_public_key() -> Result<PublicKey, KeyringError> {
        let mut keyring = Keyring::open()?;
        let user = whoami::username().unwrap_or_else(|_| "default".to_string());
        let public_key = match keyring.get_public_key(&user)? {
            Some(public_key) => public_key,
            None => keyring.rekey(&user).map(|(_, public_key)| public_key)?,
        };
        keyring.close()?;
        Ok(public_key)
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
    /// `user`. Temporary secret-byte buffers are zeroized after conversion.
    ///
    /// # Errors
    ///
    /// Returns an error if the keyring entry cannot be accessed or read.
    ///
    /// # Panics
    ///
    /// Panics if the stored secret is not exactly 32 bytes long.
    pub fn get_secret_key(&self, user: &str) -> Result<Option<SecretKey>, KeyringError> {
        match keyring_core::Entry::new(KEYRING_SERVICE, &user)?.get_secret() {
            Ok(mut secret) => {
                let secret_key = {
                    let mut secret_bytes = Zeroizing::new([0u8; 32]);
                    secret_bytes.copy_from_slice(&secret);
                    secret.zeroize();
                    SecretKey::from_bytes(&secret_bytes)
                };
                Ok(Some(secret_key))
            },
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns a user's existing secret key, or generates and stores a key pair.
    ///
    /// If a secret key already exists, its public-key file is neither checked
    /// nor repaired. Otherwise, delegates to [`Self::rekey`].
    ///
    /// # Errors
    ///
    /// Returns an error if retrieving the secret key or storing a new key pair
    /// fails.
    ///
    /// # Panics
    ///
    /// Panics if an existing stored secret is not exactly 32 bytes long.
    pub fn ensure_secret_key(&mut self, user: &str) -> Result<SecretKey, KeyringError> {
        match self.get_secret_key(user)? {
            Some(secret_key) => Ok(secret_key),
            None => self.rekey(user).map(|(secret_key, _)| secret_key),
        }
    }

    /// Generates and stores a new key pair, replacing any existing keys for `user`.
    ///
    /// Returns the secret key followed by its public key. Stores the secret in
    /// the default keyring store, then writes the text-encoded public key with
    /// a trailing newline to the user's local file.
    ///
    /// # Errors
    ///
    /// Returns an error if storing either key fails. The two writes are not
    /// atomic: if writing the public-key file fails, the new secret key remains
    /// stored and the public-key file may be missing, stale, or incomplete.
    pub fn rekey(&mut self, user: &str) -> Result<(SecretKey, PublicKey), KeyringError> {
        let secret_key = SecretKey::generate();
        {
            let secret_bytes = Zeroizing::new(secret_key.to_bytes());
            keyring_core::Entry::new(KEYRING_SERVICE, &user)?
                .set_secret(secret_bytes.as_slice())?;
        }
        let public_key: PublicKey = secret_key.public().into();
        let key_path = self.path.join(user);
        std::fs::write(&key_path, alloc::format!("{}\n", public_key))?;
        Ok((secret_key, public_key))
    }
}
