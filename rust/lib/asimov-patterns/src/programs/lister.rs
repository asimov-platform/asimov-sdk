// This is free and unencumbered software released into the public domain.

//! Collection enumeration: the lister marker trait, formats, and pagination.

use crate::{Execute, OptionSupport};
use alloc::{string::String, vec::Vec};
use bon::Builder;
use clientele::options::sort::SortKeys;

/// A directory or collection iterator that describes its entries as RDF.
///
/// A collection need not be a filesystem directory, and a successful listing
/// can contain no entries. The program defines supported URL schemes, the RDF
/// representation of each entry, and ordering guarantees. One entry can require
/// many RDF statements; pagination counts entries rather than statements or lines.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] INPUT-URL`
///
/// One absolute collection URL is required as a single argument. There is no
/// stdin payload; stdout contains RDF (`jsonl` by default). [`ListerOptions`]
/// controls output serialization and result counts. Every lister program must
/// accept and implement `--limit`: bounding its output is straightforward, and
/// independent host enforcement is a safeguard against program bugs, not a
/// replacement for supporting the option. Sorting and offset are optional
/// program capabilities because they can be more complex and can be emulated by
/// a host. Sorting precedes skipping entries, and the limit applies last.
///
/// The generic result `T` need not be a Rust iterator. A serialized line is not
/// necessarily a complete logical entry. See [`crate::programs`] for links to
/// concrete execution behavior and the [lister specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#lister
pub trait Lister<T>: Execute<T> {}

/// Declared native support for a lister program's optional operations.
///
/// Supply this separately from [`ListerOptions`]: requests and native support
/// are independent. `Default` and an empty builder leave both capabilities
/// [`Unknown`](OptionSupport::Unknown). Mandatory `--limit` and `--output`
/// support is not configurable here. A host may obtain these declarations from
/// module metadata or other knowledge of the program; this crate performs no
/// discovery. Concrete forwarding and fallback policies belong to the executor.
///
/// ```
/// use asimov_patterns::{ListerCapabilities, OptionSupport};
///
/// let capabilities = ListerCapabilities::builder()
///     .sort(OptionSupport::Unsupported)
///     .offset(OptionSupport::Supported)
///     .build();
/// assert_eq!(capabilities.sort, OptionSupport::Unsupported);
/// assert_eq!(capabilities.offset, OptionSupport::Supported);
/// assert_eq!(ListerCapabilities::builder().build(), ListerCapabilities::default());
/// assert_eq!(ListerCapabilities::default().sort, OptionSupport::Unknown);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug))]
pub struct ListerCapabilities {
    /// Native `--sort` support. Supported sort keys and RDF mapping profiles
    /// still depend on the program; this flag does not define a sorting grammar.
    #[builder(default)]
    pub sort: OptionSupport,

    /// Native `--offset` support, applied after sorting and before limiting.
    #[builder(default)]
    pub offset: OptionSupport,
}

/// Output-format and pagination requests for a [`Lister`].
///
/// `Default` leaves all optional fields unset and `other` empty: no caller
/// limit, no skipped entries, the program's default order, and its default
/// output format. The collection URL is supplied separately by the runner.
///
/// These fields express requested behavior, not program capabilities; supply
/// known native support separately using [`ListerCapabilities`].
/// `--limit` support is mandatory. `sort` and `offset` are optional capabilities,
/// and the portable sort-expression grammar is still unresolved. Use only keys
/// and syntax supported by the selected program's profile. Concrete hosts must
/// distinguish native support from emulation; see [`crate::programs`] for links
/// to the runner's currently implemented behavior.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::ListerOptions;
///
/// let options = ListerOptions::builder()
///     .limit(100)
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, /*Ord,*/ PartialEq, /*PartialOrd,*/ Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct ListerOptions {
    /// Additional arguments placed after generated options and before the URL.
    ///
    /// Each string is one literal argument, without shell expansion. The runner
    /// supplies the URL separately; do not duplicate it here. See [`crate::programs`].
    #[builder(field)]
    pub other: Vec<String>,

    /// Ordering request passed as `--sort=SORT` using [`SortKeys`]' display syntax.
    ///
    /// `SortKeys` uses a `-` prefix for descending keys. This is the SDK's
    /// representation, not a universally supported PPS grammar; the program
    /// must support both the option and the resulting expression. `None` omits
    /// the request and uses the program's documented default order.
    pub sort: Option<SortKeys>,

    /// Number of entries to skip, passed as `--offset=COUNT`.
    ///
    /// `None` omits the option (default `0`); `Some(0)` explicitly skips none.
    /// Offset is applied after sorting and before the limit. Programs may omit
    /// support for this option and must reject it if unsupported.
    pub offset: Option<usize>,

    /// Requested listing limit, passed as `--limit=COUNT` (`-n` in the CLI).
    ///
    /// `None` imposes no caller-requested limit; `Some(0)` requests no entries.
    /// All lister programs must accept and implement this option. Host enforcement
    /// is additional protection against bugs in a program's implementation.
    /// The program contract counts complete entries, not RDF statements, lines,
    /// or bytes; consult the [runner's listing behavior][implementation] for its
    /// additional line-based cap and zero-limit handling. This options value
    /// does not validate entry boundaries.
    ///
    /// [implementation]: https://docs.rs/asimov-runner/latest/asimov_runner/struct.Lister.html
    pub limit: Option<usize>,

    /// RDF serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// The option does not define how an entry maps to RDF or select a file.
    pub output: Option<String>,
}

impl<S: lister_options_builder::State> ListerOptionsBuilder<S> {
    /// Appends one literal argument to [`ListerOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`ListerOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
