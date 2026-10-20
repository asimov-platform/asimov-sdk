// This is free and unencumbered software released into the public domain.

use crate::{InvalidUrlFile, url_file::validate_url};
use alloc::{collections::BTreeSet, string::String, vec::Vec};
use core::{fmt, str::FromStr};

/// The contents of a `.urls` file: zero or more newline-terminated URLs.
///
/// Parsing preserves order, spelling, and existing duplicates. Like
/// [`crate::UrlFile`], this type validates URLs without normalizing them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UrlsFile(Vec<String>);

impl UrlsFile {
    /// Validates an entire sequence of URLs, preserving its order.
    pub fn new<I, S>(urls: I) -> Result<Self, InvalidUrlFile>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        urls.into_iter()
            .map(|url| {
                let url = url.as_ref();
                validate_url(url)?;
                Ok(String::from(url))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    /// Returns the URLs in file order, without newline terminators.
    pub fn urls(&self) -> &[String] {
        &self.0
    }

    /// Appends previously absent URLs in input order, returning the added count.
    ///
    /// Equality is exact, including URL spelling. Repeated input is added once.
    pub fn append(&mut self, urls: &Self) -> usize {
        let mut seen: BTreeSet<_> = self.0.iter().cloned().collect();
        let before = self.0.len();
        for url in &urls.0 {
            if seen.insert(url.clone()) {
                self.0.push(url.clone());
            }
        }
        self.0.len() - before
    }

    /// Removes every occurrence of the given URLs, returning the removed count.
    pub fn remove(&mut self, urls: &Self) -> usize {
        let removed: BTreeSet<_> = urls.0.iter().collect();
        let before = self.0.len();
        self.0.retain(|url| !removed.contains(url));
        before - self.0.len()
    }
}

impl FromStr for UrlsFile {
    type Err = InvalidUrlFile;

    fn from_str(contents: &str) -> Result<Self, Self::Err> {
        if contents.is_empty() {
            return Ok(Self::default());
        }
        Self::new(
            contents
                .strip_suffix('\n')
                .ok_or(InvalidUrlFile)?
                .split('\n'),
        )
    }
}

impl fmt::Display for UrlsFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for url in &self.0 {
            writeln!(f, "{url}")?;
        }
        Ok(())
    }
}
