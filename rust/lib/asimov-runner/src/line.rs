// This is free and unencumbered software released into the public domain.

//! Immutable, framing-validated JSONL lines with owned or shared storage.

use alloc::{string::String, vec::Vec};
use bytes::Bytes;
use core::{
    fmt,
    hash::{Hash, Hasher},
    ops::{Deref, Range},
};

/// Invalid framing or an invalid slice range supplied to a line constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonlLineError {
    /// An LF occurs before the end of the line. The offset is relative to the
    /// requested line, not the entire shared backing buffer.
    EmbeddedLf { offset: usize },
    /// A shared slice must satisfy `start <= end <= len`.
    InvalidRange {
        start: usize,
        end: usize,
        len: usize,
    },
}

impl fmt::Display for JsonlLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmbeddedLf { offset } => {
                write!(f, "JSONL line contains an embedded LF at byte {offset}")
            },
            Self::InvalidRange { start, end, len } => write!(
                f,
                "JSONL line range {start}..{end} is outside a {len}-byte buffer"
            ),
        }
    }
}

impl core::error::Error for JsonlLineError {}

/// Validated owned storage for [`JsonlLine::Owned`]. The private payload prevents
/// constructing a variant with unchecked bytes or mutating a validated line.
#[derive(Clone, Debug)]
pub struct OwnedJsonlLine(Vec<u8>);

impl OwnedJsonlLine {
    /// Borrows the complete stored line, including its terminator when present.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes the validated storage, returning the original allocation. Mutated
    /// bytes must go through a checked constructor to become a line again.
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Validated shared storage for [`JsonlLine::Shared`]. A line can retain a slice
/// of a larger immutable allocation. `Bytes` itself stores the view's bounds;
/// no separate backing-buffer range is retained by the line.
#[derive(Clone)]
#[repr(transparent)]
pub struct SharedJsonlLine(Bytes);

impl fmt::Debug for SharedJsonlLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Do not expose other records retained in the backing allocation.
        f.debug_tuple("SharedJsonlLine")
            .field(&self.as_bytes())
            .finish()
    }
}

impl SharedJsonlLine {
    /// Borrows only this line, not the rest of its shared backing buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes the line and returns an immutable, zero-copy view of its bytes.
    /// The view can still retain the larger backing allocation.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

/// One immutable, framing-valid JSONL line.
///
/// # Invariants
///
/// - LF may occur only as the final byte. Thus a line cannot contain multiple
///   LF-delimited records, and batches can reliably count lines.
/// - The final LF, if present, is retained; an immediately preceding CR forms
///   a CRLF terminator. Other CR bytes are preserved as content.
/// - Unterminated and blank lines are allowed, including an empty byte sequence
///   representing a blank unterminated line. An empty line is not EOF or an empty
///   batch. Graph input appends an LF to unterminated lines when writing them.
/// - Shared storage is the exact validated `Bytes` view; surrounding bytes are
///   not exposed even when the view retains a larger allocation.
///
/// These are framing guarantees, not UTF-8, JSON, or JSON-LD validation. Public
/// constructors check framing and expose no mutable access. Variant payloads
/// are validated wrappers rather than raw buffers, so direct enum construction
/// cannot bypass the checks. Consuming extraction methods return unvalidated
/// buffers; reconstructing a line requires checking them again.
///
/// Owned clones copy their bytes. Shared clones and slices share storage without
/// copying payload bytes. Equality and hashing compare the full stored bytes,
/// including terminators, independently of storage kind. Retaining a small shared
/// line can retain a large allocation; use [`into_compact`](Self::into_compact)
/// for long-lived, sparse retention.
///
/// ```
/// use asimov_runner::{Bytes, JsonlLine, JsonlLineError};
/// let owned = JsonlLine::owned(b"{}\r\n".to_vec())?;
/// let shared = JsonlLine::shared(Bytes::from_static(b"{}\r\n"))?;
/// assert_eq!(owned, shared);
/// assert_eq!(shared.content(), b"{}");
/// assert!(shared.is_terminated());
/// assert!(JsonlLine::owned(b"{}\n[]\n".to_vec()).is_err());
/// # Ok::<(), JsonlLineError>(())
/// ```
///
/// Raw variant construction is intentionally unavailable:
///
/// ```compile_fail
/// use asimov_runner::JsonlLine;
/// let invalid = JsonlLine::Owned(b"first\nsecond".to_vec());
/// ```
///
/// Validated bytes cannot be changed in place:
///
/// ```compile_fail
/// use asimov_runner::JsonlLine;
/// let mut line = JsonlLine::owned(b"abc".to_vec()).unwrap();
/// line[0] = b'\n';
/// ```
#[derive(Clone, Debug)]
pub enum JsonlLine {
    /// Exclusively owned, validated bytes. Cloning copies the payload.
    Owned(OwnedJsonlLine),
    /// Shared, validated bytes. Cloning does not copy the payload.
    Shared(SharedJsonlLine),
}

impl JsonlLine {
    /// Validates and takes ownership of a vector without copying its bytes.
    pub fn owned(bytes: Vec<u8>) -> Result<Self, JsonlLineError> {
        validate(&bytes)?;
        Ok(Self::Owned(OwnedJsonlLine(bytes)))
    }

    /// Validates an immutable byte buffer without copying its bytes.
    pub fn shared(bytes: Bytes) -> Result<Self, JsonlLineError> {
        validate(&bytes)?;
        Ok(Self::Shared(SharedJsonlLine(bytes)))
    }

    /// Validates a line within an immutable backing buffer. Bytes outside `range`
    /// need not be a single line. Stores only the resulting `Bytes` view, without
    /// copying its payload. That view can still retain the original allocation.
    pub fn shared_slice(backing: Bytes, range: Range<usize>) -> Result<Self, JsonlLineError> {
        let bytes = backing
            .get(range.clone())
            .ok_or(JsonlLineError::InvalidRange {
                start: range.start,
                end: range.end,
                len: backing.len(),
            })?;
        validate(bytes)?;
        Ok(Self::Shared(SharedJsonlLine(backing.slice(range))))
    }

    /// Checks framing before copying borrowed bytes into owned storage.
    pub fn copy_from_slice(bytes: &[u8]) -> Result<Self, JsonlLineError> {
        validate(bytes)?;
        Ok(Self::Owned(OwnedJsonlLine(bytes.to_vec())))
    }

    /// Borrows the original bytes, including any LF/CRLF terminator.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Owned(line) => line.as_bytes(),
            Self::Shared(line) => line.as_bytes(),
        }
    }

    /// Borrows content excluding an LF or CRLF terminator, without trimming
    /// whitespace or removing a bare CR from an unterminated line.
    pub fn content(&self) -> &[u8] {
        let bytes = self.as_bytes();
        match bytes.strip_suffix(b"\n") {
            Some(content) => content.strip_suffix(b"\r").unwrap_or(content),
            None => bytes,
        }
    }

    /// Whether this line has an LF or CRLF terminator.
    pub fn is_terminated(&self) -> bool {
        self.as_bytes().ends_with(b"\n")
    }

    /// Stored byte length, including any terminator.
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Whether this is a blank unterminated line with no stored bytes.
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    /// Extracts owned bytes. Owned storage is moved; shared storage is copied.
    pub fn into_vec(self) -> Vec<u8> {
        match self {
            Self::Owned(line) => line.into_vec(),
            Self::Shared(line) => line.as_bytes().to_vec(),
        }
    }

    /// Extracts immutable bytes without copying payload data. Owned vectors are
    /// adopted by `Bytes`; shared lines return their existing `Bytes` view.
    pub fn into_bytes(self) -> Bytes {
        match self {
            Self::Owned(line) => Bytes::from(line.into_vec()),
            Self::Shared(line) => line.into_bytes(),
        }
    }

    /// Switches to shared storage without copying payload bytes or rescanning.
    pub fn into_shared(self) -> Self {
        match self {
            Self::Shared(_) => self,
            Self::Owned(line) => Self::Shared(SharedJsonlLine(Bytes::from(line.into_vec()))),
        }
    }

    /// Copies this line into an independent, compact owned allocation, releasing
    /// its reference to any larger backing buffer. Stored bytes are unchanged.
    pub fn into_compact(self) -> Self {
        Self::Owned(OwnedJsonlLine(self.as_bytes().to_vec()))
    }

    /// Only framing/batch code may use this path: it has already validated this
    /// view. Release builds avoid rescanning each extracted line.
    #[cfg(feature = "std")]
    pub(crate) fn framed(bytes: Bytes) -> Self {
        debug_assert!(validate(&bytes).is_ok());
        Self::Shared(SharedJsonlLine(bytes))
    }
}

fn validate(bytes: &[u8]) -> Result<(), JsonlLineError> {
    if let Some(offset) = memchr::memchr(b'\n', bytes) {
        if offset + 1 != bytes.len() {
            return Err(JsonlLineError::EmbeddedLf { offset });
        }
    }
    Ok(())
}

impl TryFrom<Vec<u8>> for JsonlLine {
    type Error = JsonlLineError;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::owned(bytes)
    }
}

impl TryFrom<Bytes> for JsonlLine {
    type Error = JsonlLineError;
    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
        Self::shared(bytes)
    }
}

impl TryFrom<String> for JsonlLine {
    type Error = JsonlLineError;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::owned(text.into_bytes())
    }
}

impl AsRef<[u8]> for JsonlLine {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Deref for JsonlLine {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl PartialEq for JsonlLine {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for JsonlLine {}
impl Hash for JsonlLine {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use alloc::{format, vec};
    use std::{
        collections::hash_map::DefaultHasher,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    #[test]
    fn constructors_check_framing_in_both_storage_modes() {
        for (bytes, offset) in [
            (b"\nx".as_slice(), 0),
            (b"a\nb", 1),
            (b"\n\n", 0),
            (b"a\r\nb", 2),
            (b"a\n\r\n", 1),
        ] {
            let expected = JsonlLineError::EmbeddedLf { offset };
            assert_eq!(JsonlLine::owned(bytes.to_vec()).unwrap_err(), expected);
            assert_eq!(
                JsonlLine::shared(Bytes::copy_from_slice(bytes)).unwrap_err(),
                expected
            );
            assert_eq!(JsonlLine::copy_from_slice(bytes).unwrap_err(), expected);
        }
        assert!(JsonlLine::try_from(String::from("first\nsecond")).is_err());
    }

    #[test]
    fn valid_lines_preserve_bytes_content_and_terminators() {
        for (bytes, content, terminated) in [
            (b"".as_slice(), b"".as_slice(), false),
            (b"\n", b"", true),
            (b"\r\n", b"", true),
            (b"{}", b"{}", false),
            (b"{}\n", b"{}", true),
            (b"{}\r\n", b"{}", true),
            (b"a\rb\r", b"a\rb\r", false),
            (b" \t{}\t \r\n", b" \t{}\t ", true),
            (b"\xff\n", b"\xff", true),
        ] {
            let owned = JsonlLine::owned(bytes.to_vec()).unwrap();
            let shared = JsonlLine::shared(Bytes::copy_from_slice(bytes)).unwrap();
            assert_eq!(owned, shared);
            for line in [owned, shared] {
                assert_eq!(line.as_bytes(), bytes);
                assert_eq!(line.content(), content);
                assert_eq!(line.is_terminated(), terminated);
                assert_eq!(line.len(), bytes.len());
                assert_eq!(line.is_empty(), bytes.is_empty());
            }
        }
    }

    #[test]
    fn shared_ranges_are_checked_and_errors_are_relative_to_the_line() {
        assert_eq!(
            core::mem::size_of::<SharedJsonlLine>(),
            core::mem::size_of::<Bytes>()
        );
        let backing = Bytes::from_static(b"prefix\n{}\r\nsuffix\n");
        let line = JsonlLine::shared_slice(backing.clone(), 7..11).unwrap();
        let JsonlLine::Shared(payload) = &line else {
            unreachable!()
        };
        assert_eq!(payload.0.len(), 4, "Bytes itself is the exact line view");
        assert_eq!(payload.0.as_ptr(), backing[7..11].as_ptr());
        assert_eq!(line.content(), b"{}");
        assert_eq!(line.clone().into_bytes().as_ref(), b"{}\r\n");
        assert_eq!(
            JsonlLine::shared_slice(backing.clone(), 7..12).unwrap_err(),
            JsonlLineError::EmbeddedLf { offset: 3 }
        );
        for range in [9..8, 0..backing.len() + 1, usize::MAX..usize::MAX] {
            assert_eq!(
                JsonlLine::shared_slice(backing.clone(), range.clone()).unwrap_err(),
                JsonlLineError::InvalidRange {
                    start: range.start,
                    end: range.end,
                    len: backing.len()
                }
            );
        }
        let debug = format!("{line:?}");
        assert_eq!(debug, "Shared(SharedJsonlLine([123, 125, 13, 10]))");
    }

    #[test]
    fn ownership_conversions_and_clones_have_documented_copy_behavior() {
        let mut data = Vec::with_capacity(1024);
        data.extend_from_slice(b"{}\n");
        let pointer = data.as_ptr();
        let owned = JsonlLine::owned(data).unwrap();
        let copied = owned.clone();
        assert_ne!(copied.as_bytes().as_ptr(), pointer);
        let shared = owned.into_shared();
        assert!(matches!(shared, JsonlLine::Shared(_)));
        assert_eq!(shared.as_bytes().as_ptr(), pointer);
        let clone = shared.clone();
        assert_eq!(clone.as_bytes().as_ptr(), pointer);
        assert_eq!(shared, copied);
        let hash = |line: &JsonlLine| {
            let mut h = DefaultHasher::new();
            line.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&shared), hash(&copied));
        assert_eq!(shared.into_bytes().as_ptr(), pointer);
        assert_eq!(clone.into_vec(), b"{}\n");
        assert_ne!(JsonlLine::owned(b"{}".to_vec()).unwrap(), copied);
    }

    struct Owner {
        bytes: Vec<u8>,
        dropped: Arc<AtomicBool>,
    }
    impl AsRef<[u8]> for Owner {
        fn as_ref(&self) -> &[u8] {
            &self.bytes
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn compact_line_releases_a_large_backing_owner() {
        let dropped = Arc::new(AtomicBool::new(false));
        let backing = Bytes::from_owner(Owner {
            bytes: vec![b'x'; 256 * 1024],
            dropped: dropped.clone(),
        });
        let line = JsonlLine::shared_slice(backing, 123..127).unwrap();
        assert!(!dropped.load(Ordering::SeqCst));
        let line = line.into_compact();
        assert!(dropped.load(Ordering::SeqCst));
        let JsonlLine::Owned(line) = line else {
            panic!("compact lines use owned storage")
        };
        let bytes = line.into_vec();
        assert_eq!(bytes, b"xxxx");
        assert_eq!(bytes.capacity(), bytes.len());
    }
}
