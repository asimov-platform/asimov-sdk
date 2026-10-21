// This is free and unencumbered software released into the public domain.

use crate::{InvalidUriFile, uri_file::validate_uri};
use alloc::{collections::BTreeSet, string::String, vec::Vec};
use core::{fmt, str::FromStr};

/// Owned contents of a `.uris` file: zero or more LF-terminated absolute IRIs.
///
/// Parsing preserves order, spelling, and existing duplicates. The URI-list
/// format and validation follow [`crate::UriFile`], including UTF-8 IRIs, LF
/// terminators, and no comment support.
/// With the `tokio` feature, `UrisReader` reads large files incrementally without
/// allocating owned strings for their IRIs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UrisFile(Vec<String>);

impl UrisFile {
    /// Validates an entire sequence of IRIs, preserving its order.
    pub fn new<I, S>(uris: I) -> Result<Self, InvalidUriFile>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        uris.into_iter()
            .map(|uri| {
                let uri = uri.as_ref();
                validate_uri(uri)?;
                Ok(String::from(uri))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    /// Returns the IRIs in file order, without newline terminators.
    pub fn uris(&self) -> &[String] {
        &self.0
    }

    /// Appends previously absent IRIs in input order, returning the added count.
    ///
    /// Equality is exact, including IRI spelling. Repeated input is added once.
    pub fn append(&mut self, uris: &Self) -> usize {
        let mut seen: BTreeSet<_> = self.0.iter().cloned().collect();
        let before = self.0.len();
        for uri in &uris.0 {
            if seen.insert(uri.clone()) {
                self.0.push(uri.clone());
            }
        }
        self.0.len() - before
    }

    /// Removes every occurrence of the given IRIs, returning the removed count.
    pub fn remove(&mut self, uris: &Self) -> usize {
        let removed: BTreeSet<_> = uris.0.iter().collect();
        let before = self.0.len();
        self.0.retain(|uri| !removed.contains(uri));
        before - self.0.len()
    }
}

impl FromStr for UrisFile {
    type Err = InvalidUriFile;

    fn from_str(contents: &str) -> Result<Self, Self::Err> {
        if contents.is_empty() {
            return Ok(Self::default());
        }
        Self::new(
            contents
                .strip_suffix('\n')
                .ok_or(InvalidUriFile)?
                .split('\n'),
        )
    }
}

impl fmt::Display for UrisFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for uri in &self.0 {
            writeln!(f, "{uri}")?;
        }
        Ok(())
    }
}
