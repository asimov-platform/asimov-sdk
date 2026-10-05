// This is free and unencumbered software released into the public domain.

use crate::KeyringError;
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use keyring_core::{CredentialStore, Entry};
use std::sync::Mutex;

/// Shared ownership and in-process coordination for one backend instance.
pub(crate) struct Store {
    backend: Arc<CredentialStore>,
    users: Mutex<BTreeMap<String, Weak<Mutex<()>>>>,
}

impl Store {
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

    pub(crate) fn user_lock(&self, user: &str) -> Result<Arc<Mutex<()>>, KeyringError> {
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

fn platform_backend() -> keyring_core::Result<Arc<CredentialStore>> {
    #[cfg(target_vendor = "apple")]
    let store = apple_native_keyring_store::keychain::Store::new()?;
    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new()?;
    #[cfg(target_os = "linux")]
    let store = linux_keyutils_keyring_store::Store::new()?;
    #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_os = "linux")))]
    let store = keyring_core::mock::Store::new()?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};

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
