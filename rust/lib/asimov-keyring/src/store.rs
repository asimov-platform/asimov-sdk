// This is free and unencumbered software released into the public domain.

use crate::KeyringError;
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use asimov_core::crates::cap_std::fs::Dir;
use iroh_base::SecretKey;
use keyring_core::{CredentialStore, Entry};
use secrecy::zeroize::Zeroizing;
use std::sync::Mutex;

/// Shared ownership and in-process coordination for one backend instance.
pub(crate) struct Store {
    backend: Arc<CredentialStore>,
    users: Mutex<BTreeMap<String, Weak<Mutex<()>>>>,
}

impl Store {
    pub(crate) fn configured(_root: &Dir) -> Result<Arc<Self>, KeyringError> {
        match std::env::var_os("ASIMOV_KEYRING_BACKEND").as_deref() {
            #[cfg(target_os = "linux")]
            None => Self::automatic(_root, Self::platform),
            #[cfg(not(target_os = "linux"))]
            None => Self::platform(),
            Some(value) if value == "native" => Self::platform(),
            #[cfg(unix)]
            Some(value) if value == "file" => Self::file(_root),
            _ => Err(keyring_core::Error::Invalid(
                "ASIMOV_KEYRING_BACKEND".into(),
                "expected native, or file on Unix".into(),
            )
            .into()),
        }
    }

    #[cfg(unix)]
    fn file(root: &Dir) -> Result<Arc<Self>, KeyringError> {
        Self::shared(Arc::new(crate::file_store::FileStore::new(root)?))
    }

    #[cfg(target_os = "linux")]
    fn automatic(
        root: &Dir,
        native: impl FnOnce() -> Result<Arc<Self>, KeyringError>,
    ) -> Result<Arc<Self>, KeyringError> {
        // Retain the file identity if a Secret Service later becomes available.
        // Invalid or inaccessible file storage must not select a new identity.
        match root.symlink_metadata(crate::file_store::DIRECTORY) {
            Ok(_) => return Self::file(root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(error.into()),
        }

        match native() {
            Err(error) if secret_service_absent(&error) => Self::file(root),
            other => other,
        }
    }

    pub(crate) fn platform() -> Result<Arc<Self>, KeyringError> {
        static PLATFORM: Mutex<Weak<Store>> = Mutex::new(Weak::new());
        Self::get_or_init(&PLATFORM, platform_backend)
    }

    fn get_or_init(
        slot: &Mutex<Weak<Self>>,
        create: impl FnOnce() -> keyring_core::Result<Arc<CredentialStore>>,
    ) -> Result<Arc<Self>, KeyringError> {
        let mut slot = slot.lock().map_err(|_| KeyringError::LockPoisoned)?;
        if let Some(store) = slot.upgrade() {
            return Ok(store);
        }
        let store = Self::shared(create()?)?;
        *slot = Arc::downgrade(&store);
        Ok(store)
    }

    pub(crate) fn shared(backend: Arc<CredentialStore>) -> Result<Arc<Self>, KeyringError> {
        // Weak references coordinate separately opened handles without retaining
        // their backends after the last handle has been released.
        static STORES: Mutex<Vec<Weak<Store>>> = Mutex::new(Vec::new());
        let mut stores = STORES.lock().map_err(|_| KeyringError::LockPoisoned)?;
        stores.retain(|store| store.strong_count() != 0);
        for store in stores.iter().filter_map(Weak::upgrade) {
            if Arc::ptr_eq(&store.backend, &backend) {
                return Ok(store);
            }
        }
        let store = Arc::new(Self {
            backend,
            users: Mutex::new(BTreeMap::new()),
        });
        stores.push(Arc::downgrade(&store));
        Ok(store)
    }

    pub(crate) fn entry(&self, user: &str) -> keyring_core::Result<Entry> {
        self.backend.build(crate::KEYRING_SERVICE, user, None)
    }

    pub(crate) fn secret_key(&self, stored: &[u8]) -> Result<SecretKey, KeyringError> {
        let corrupt = || KeyringError::CorruptSecret {
            length: stored.len(),
        };
        #[cfg(target_os = "linux")]
        if self.is_secret_service() {
            let mut secret = Zeroizing::new([0; 32]);
            return match bs58::decode(stored).onto(&mut secret[..]) {
                Ok(32) => Ok(SecretKey::from_bytes(&secret)),
                _ => Err(corrupt()),
            };
        }
        stored
            .try_into()
            .map(SecretKey::from_bytes)
            .map_err(|_| corrupt())
    }

    pub(crate) fn set_secret_key(
        &self,
        entry: &Entry,
        secret_key: &SecretKey,
    ) -> keyring_core::Result<()> {
        let secret = Zeroizing::new(secret_key.to_bytes());
        // KDE Wallet stores only UTF-8 secrets.
        #[cfg(target_os = "linux")]
        if self.is_secret_service() {
            let mut encoded = Zeroizing::new([0; 64]);
            let length = bs58::encode(secret.as_slice())
                .onto(&mut encoded[..])
                .expect("Base58 encodes 32 bytes in at most 44 characters");
            return entry.set_secret(&encoded[..length]);
        }
        entry.set_secret(secret.as_slice())
    }

    #[cfg(target_os = "linux")]
    fn is_secret_service(&self) -> bool {
        self.backend
            .as_any()
            .is::<zbus_secret_service_keyring_store::Store>()
    }

    // Keep the file lock through the whole read/modify/cache/rollback operation.
    // Locking individual credential calls would race concurrent first use.
    pub(crate) fn file_lock(&self, _user: &str) -> Result<Option<std::fs::File>, KeyringError> {
        #[cfg(unix)]
        if let Some(store) = self
            .backend
            .as_any()
            .downcast_ref::<crate::file_store::FileStore>()
        {
            return Ok(Some(store.lock(_user)?));
        }
        Ok(None)
    }

    pub(crate) fn user_lock(&self, user: &str) -> Result<Arc<Mutex<()>>, KeyringError> {
        asimov_core::validate_filename_component(user)?;
        let mut users = self.users.lock().map_err(|_| KeyringError::LockPoisoned)?;
        users.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = users.get(user).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        users.insert(user.to_string(), Arc::downgrade(&lock));
        Ok(lock)
    }
}

// Only a missing session bus or Secret Service means no identity can be stored
// there. Other failures, such as denied access or a service restarting, may hide
// an existing identity, so they must not select the file store.
#[cfg(target_os = "linux")]
fn secret_service_absent(error: &KeyringError) -> bool {
    use secret_service::Error;
    let KeyringError::KeyringError(keyring_core::Error::PlatformFailure(error)) = error else {
        return false;
    };
    // `Error::Unavailable` also covers a malformed bus address, a configuration
    // error rather than absence.
    match error.downcast_ref::<Error>() {
        Some(Error::Zbus(zbus::Error::Connection(error, _) | zbus::Error::InputOutput(error))) => {
            error.kind() == std::io::ErrorKind::NotFound
        },
        Some(Error::Zbus(zbus::Error::MethodError(name, ..))) => {
            name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown"
        },
        Some(Error::ZbusFdo(zbus::fdo::Error::ServiceUnknown(_))) => true,
        _ => false,
    }
}

fn platform_backend() -> keyring_core::Result<Arc<CredentialStore>> {
    #[cfg(target_vendor = "apple")]
    let store = apple_native_keyring_store::keychain::Store::new()?;
    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new()?;
    #[cfg(target_os = "linux")]
    let store = zbus_secret_service_keyring_store::Store::new()?;
    #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_os = "linux")))]
    let store = keyring_core::mock::Store::new()?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(target_os = "linux")]
    #[test]
    fn automatically_falls_back_without_a_secret_service() {
        use alloc::boxed::Box;

        let path = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(
            path.path(),
            asimov_core::crates::cap_std::ambient_authority(),
        )
        .unwrap();
        let unavailable = || {
            let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
            Err(
                keyring_core::Error::PlatformFailure(Box::new(secret_service::Error::Zbus(
                    zbus::Error::InputOutput(Arc::new(missing)),
                )))
                .into(),
            )
        };
        let store = Store::automatic(&root, unavailable).unwrap();
        assert!(store.backend.as_any().is::<crate::file_store::FileStore>());
        assert!(path.path().join(".keyring").is_dir());
        drop(store);

        // A Secret Service appearing later must not switch an existing identity.
        let reopened = Store::automatic(&root, || panic!("file store already selected")).unwrap();
        assert!(
            reopened
                .backend
                .as_any()
                .is::<crate::file_store::FileStore>()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn automatic_selection_keeps_accessible_native_storage_and_other_errors() {
        use alloc::boxed::Box;

        let path = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(
            path.path(),
            asimov_core::crates::cap_std::ambient_authority(),
        )
        .unwrap();
        let native = Store::shared(keyring_core::mock::Store::new().unwrap()).unwrap();
        let selected = Store::automatic(&root, || Ok(native.clone())).unwrap();
        assert!(Arc::ptr_eq(&selected, &native));
        assert!(matches!(
            selected.entry("default").unwrap().get_secret(),
            Err(keyring_core::Error::NoEntry)
        ));

        assert!(matches!(
            Store::automatic(&root, || Err(KeyringError::LockPoisoned)),
            Err(KeyringError::LockPoisoned)
        ));
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(matches!(
            Store::automatic(&root, || Err(keyring_core::Error::PlatformFailure(
                Box::new(secret_service::Error::Zbus(zbus::Error::InputOutput(
                    Arc::new(denied)
                )))
            )
            .into())),
            Err(KeyringError::KeyringError(
                keyring_core::Error::PlatformFailure(_)
            ))
        ));
        assert!(matches!(
            Store::automatic(&root, || Err(keyring_core::Error::Invalid(
                "test".into(),
                "invalid configuration".into(),
            )
            .into())),
            Err(KeyringError::KeyringError(keyring_core::Error::Invalid(
                _,
                _
            )))
        ));
        assert!(!path.path().join(".keyring").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn automatic_selection_does_not_bypass_invalid_file_storage() {
        let path = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(
            path.path(),
            asimov_core::crates::cap_std::ambient_authority(),
        )
        .unwrap();
        root.write(".keyring", b"untouched").unwrap();
        assert!(matches!(
            Store::automatic(&root, || panic!("must not fall back to native")),
            Err(KeyringError::IoError(_))
        ));
        assert_eq!(root.read(".keyring").unwrap(), b"untouched");
    }

    #[test]
    fn overlapping_opens_initialize_once_and_release_the_backend() {
        let slot = Mutex::new(Weak::new());
        let count = AtomicUsize::new(0);
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        Store::get_or_init(&slot, || {
                            count.fetch_add(1, Ordering::SeqCst);
                            Ok(keyring_core::mock::Store::new()?)
                        })
                        .unwrap()
                    })
                })
                .collect();
            let stores: Vec<_> = tasks.into_iter().map(|task| task.join().unwrap()).collect();
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert!(stores.iter().all(|store| Arc::ptr_eq(store, &stores[0])));
            let backend = Arc::downgrade(&stores[0].backend);
            drop(stores);
            assert!(backend.upgrade().is_none());
        });
        assert!(slot.lock().unwrap().upgrade().is_none());
        let reopened = Store::get_or_init(&slot, || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(keyring_core::mock::Store::new()?)
        })
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        drop(reopened);
    }

    #[test]
    fn failed_initialization_can_be_retried() {
        let slot = Mutex::new(Weak::new());
        let result = Store::get_or_init(&slot, || Err(keyring_core::Error::NoDefaultStore));
        assert!(matches!(result, Err(KeyringError::KeyringError(_))));
        assert!(slot.lock().unwrap().upgrade().is_none());
        assert!(Store::get_or_init(&slot, || Ok(keyring_core::mock::Store::new()?)).is_ok());
    }

    #[test]
    fn same_user_shares_a_lock_without_blocking_other_users() {
        let backend = keyring_core::mock::Store::new().unwrap();
        let first = Store::shared(backend.clone()).unwrap();
        let second = Store::shared(backend).unwrap();
        let alice = first.user_lock("alice").unwrap();
        let alice_again = second.user_lock("alice").unwrap();
        let bob = second.user_lock("bob").unwrap();
        assert!(Arc::ptr_eq(&alice, &alice_again));
        let _guard = alice.lock().unwrap();
        assert!(matches!(
            alice_again.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        assert!(bob.try_lock().is_ok());
    }
}
