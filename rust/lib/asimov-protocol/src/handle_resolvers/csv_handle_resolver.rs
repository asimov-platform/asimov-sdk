// This is free and unencumbered software released into the public domain.

use crate::{PeerId, ResolveHandle};
use alloc::string::String;
use asimov_id::Handle;
use core::str::FromStr;
use csv_async::{AsyncReader, AsyncReaderBuilder, StringRecord};
use futures_lite::{Stream, StreamExt, pin, stream};
use iroh::EndpointId;
use std::io::{Error, Result};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet as Set;

#[cfg(feature = "std")]
use std::collections::HashSet as Set;

/// A CSV file resolver from ASIMOV handles to peer IDs.
///
/// The format of the CSV file is simply `handle,peer_id`.
/// The records should be sorted for efficient resolution.
/// The file is read line-by-line, so it can be very large.
pub struct CsvHandleResolver(AsyncReader<File>);

impl CsvHandleResolver {
    /// Opens a CSV file for resolving ASIMOV handles.
    pub async fn open(path: &str) -> std::io::Result<Self> {
        let file = File::open(path).await?;
        let reader = AsyncReaderBuilder::new()
            .has_headers(false)
            .create_reader(file);
        Ok(Self::from(reader))
    }

    pub fn handles(&mut self) -> impl Stream<Item = Result<Handle>> + Send {
        async_stream::stream! {
            let mut handles = Set::new();
            let records = self.records();
            pin!(records);
            while let Some(record) = records.next().await {
                let (handle, _) = record?;
                if handles.contains(&handle) {
                    continue; // skip duplicate handles
                }
                handles.insert(handle.clone());
                yield Ok(handle);
            }
        }
    }

    pub fn records(&mut self) -> impl Stream<Item = Result<(Handle, PeerId)>> + Send {
        async_stream::stream! {
            self.0.rewind().await?;
            let mut record = StringRecord::new();
            while self.0.read_record(&mut record).await.map_err(Error::other)? {
                let Some(record_handle) = record.get(0) else {
                    continue; // skip invalid records
                };
                let Some(record_endpoint) = record.get(1) else {
                    continue; // skip invalid records
                };
                let Ok(handle) = record_handle.parse::<Handle>() else {
                    continue; // skip invalid handles
                };
                let Ok(endpoint) = record_endpoint.parse::<PeerId>() else {
                    continue; // skip invalid peer IDs
                };
                yield Ok((handle, endpoint));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{boxed::Box, format, vec::Vec};

    async fn fixture(bytes: &[u8]) -> (tempfile::TempDir, CsvHandleResolver) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peers.csv");
        tokio::fs::write(&path, bytes).await.unwrap();
        let resolver = CsvHandleResolver::open(path.to_str().unwrap())
            .await
            .unwrap();
        (dir, resolver)
    }

    #[tokio::test]
    async fn empty_csv_yields_no_records() {
        let (_dir, mut resolver) = fixture(b"").await;
        assert!(Box::pin(resolver.records()).next().await.is_none());
    }

    #[tokio::test]
    async fn csv_errors_survive_record_and_handle_resolution() {
        let peer: PeerId = crate::SecretKey::from_bytes(&[1; 32]).public().into();
        for suffix in [b"example,extra,field\n".as_slice(), b"\xff,invalid\n"] {
            let mut bytes = format!("example,{peer}\n").into_bytes();
            bytes.extend_from_slice(suffix);
            let (_dir, mut resolver) = fixture(&bytes).await;

            let records: Vec<_> = Box::pin(resolver.records()).collect().await;
            assert_eq!(records.len(), 2);
            assert_eq!(records[0].as_ref().unwrap().1, peer);
            let error = records[1].as_ref().unwrap_err();
            assert!(error.get_ref().unwrap().is::<csv_async::Error>());

            let handles: Vec<_> = Box::pin(resolver.handles()).collect().await;
            assert_eq!(handles.len(), 2);
            assert!(handles[1].is_err());

            let handle: Handle = "example".parse().unwrap();
            let peers: Vec<_> = Box::pin(resolver.resolve_handle(handle)).collect().await;
            assert_eq!(peers.len(), 2);
            assert_eq!(*peers[0].as_ref().unwrap(), peer);
            assert!(peers[1].is_err());
        }
    }

    #[tokio::test]
    async fn duplicate_csv_records_are_deduplicated_during_resolution() {
        let peer: PeerId = crate::SecretKey::from_bytes(&[1; 32]).public().into();
        let bytes = format!("example,{peer}\nexample,{peer}\n");
        let (_dir, mut resolver) = fixture(bytes.as_bytes()).await;
        let handles: Vec<_> = Box::pin(resolver.handles()).collect().await;
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].as_ref().unwrap().as_str(), "example");
        let handle: Handle = "example".parse().unwrap();
        let peers: Vec<_> = Box::pin(resolver.resolve_handle(handle)).collect().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(*peers[0].as_ref().unwrap(), peer);
    }
}

impl From<AsyncReader<File>> for CsvHandleResolver {
    fn from(reader: AsyncReader<File>) -> Self {
        Self(reader)
    }
}

impl ResolveHandle for CsvHandleResolver {
    type Error = std::io::Error;

    /// Resolves a handle into a set of endpoint IDs.
    fn resolve_handle(
        &mut self,
        handle: impl Into<Handle>,
    ) -> impl Stream<Item = Result<PeerId>> + Send {
        let handle = handle.into();
        async_stream::stream! {
            let mut endpoints = Set::new();
            let records = self.records();
            pin!(records);
            while let Some(record) = records.next().await {
                let (record_handle, record_endpoint) = record?;
                if record_handle != handle {
                    continue; // skip records that don't match
                }
                if endpoints.contains(&record_endpoint) {
                    continue; // skip duplicate endpoints
                }
                endpoints.insert(record_endpoint.clone());
                yield Ok(record_endpoint);
            }
        }
    }
}
