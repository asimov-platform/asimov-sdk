// This is free and unencumbered software released into the public domain.

use crate::PeerId;
use alloc::boxed::Box;
use asimov_id::{Handle, Id};
use core::pin::Pin;
use futures_lite::{Stream, StreamExt, stream};

/// A resolver for ASIMOV handles (e.g., "Ⓐjhacker").
pub trait ResolveHandle {
    type Error: core::fmt::Debug + Send;

    /// Resolves an ASIMOV ID and yields a random known peer ID.
    /// Ignores any erroneous results, sampling from only successful results.
    ///
    /// Returns `None` if no successful results are available.
    /// Available only with the `random` feature.
    #[cfg(feature = "random")]
    fn resolve_random(
        &mut self,
        id: impl Into<Id>,
    ) -> impl Future<Output = Result<Option<PeerId>, Self::Error>> {
        async move {
            let mut stream = Box::pin(self.resolve_all(id));
            let mut selected = None;
            let mut count = 0;
            while let Some(result) = stream.next().await {
                let Ok(result) = result else {
                    continue; // ignore errors silently
                };
                count += 1;
                // Reservoir sampling retains one peer regardless of stream size.
                if fastrand::usize(..count) == 0 {
                    selected = Some(result);
                }
            }
            Ok(selected)
        }
    }

    /// Resolves an ASIMOV ID and yields only the first known peer ID.
    /// Ignores any initial erroneous results, returning the first successful result.
    fn resolve_first(
        &mut self,
        id: impl Into<Id>,
    ) -> impl Future<Output = Result<Option<PeerId>, Self::Error>> {
        async move {
            let mut stream = Box::pin(self.resolve_all(id));
            while let Some(result) = stream.next().await {
                let Ok(result) = result else {
                    continue; // ignore errors silently
                };
                return Ok(Some(result));
            }
            Ok(None)
        }
    }

    /// Resolves an ASIMOV ID into a stream of all known peer IDs.
    fn resolve_all(
        &mut self,
        id: impl Into<Id>,
    ) -> impl Stream<Item = Result<PeerId, Self::Error>> + Send {
        let output: Pin<Box<dyn Stream<Item = Result<PeerId, Self::Error>> + Send>> =
            match id.into() {
                Id::Handle(handle) => Box::pin(self.resolve_handle(handle)),
                Id::PublicKey(key) => Box::pin(stream::once(Ok(key))),
            };
        output
    }

    /// Resolves an ASIMOV handle into a stream of all known peer IDs.
    ///
    /// This is the only method that trait implementors must provide.
    fn resolve_handle(
        &mut self,
        _handle: impl Into<Handle>,
    ) -> impl Stream<Item = Result<PeerId, Self::Error>> + Send {
        Box::pin(stream::empty())
    }
}

#[cfg(all(test, feature = "random"))]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    struct Fixture(Vec<Result<PeerId, ()>>);

    impl ResolveHandle for Fixture {
        type Error = ();

        fn resolve_handle(
            &mut self,
            _: impl Into<Handle>,
        ) -> impl Stream<Item = Result<PeerId, Self::Error>> + Send {
            stream::iter(core::mem::take(&mut self.0))
        }
    }

    #[tokio::test]
    async fn random_resolution_handles_empty_and_error_only_streams() {
        for results in [vec![], vec![Err(()), Err(())]] {
            let handle = Id::Handle("example".parse().unwrap());
            assert_eq!(Fixture(results).resolve_random(handle).await.unwrap(), None);
        }
    }

    #[tokio::test]
    async fn random_resolution_selects_only_successful_peers() {
        let first: PeerId = crate::SecretKey::from_bytes(&[1; 32]).public().into();
        let second: PeerId = crate::SecretKey::from_bytes(&[2; 32]).public().into();
        let handle = Id::Handle("example".parse().unwrap());
        assert_eq!(
            Fixture(vec![Err(()), Ok(first), Err(())])
                .resolve_random(handle.clone())
                .await
                .unwrap(),
            Some(first)
        );
        for _ in 0..16 {
            let selected = Fixture(vec![Err(()), Ok(first), Ok(first), Ok(second)])
                .resolve_random(handle.clone())
                .await
                .unwrap();
            assert!(selected == Some(first) || selected == Some(second));
        }
    }
}
