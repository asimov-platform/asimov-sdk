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
/// controls output serialization and pagination. Native support for `--sort`,
/// `--offset`, `--before`, `--after`, and `--limit` is described by
/// [`ListerCapabilities`]; none of these options is required of every program.
/// A host can enforce a limit independently of native support.
///
/// Pagination can use either numeric `offset` plus `limit`, or URI cursor bounds
/// `before`/`after` plus `limit`. Cursors identify entries by their JSON-LD `@id`,
/// not by row number or an opaque encoded token. Bounds are exclusive and refer
/// to positions in the selected sort order, not lexical ordering of the URIs.
/// Both bounds may specify an interval; numeric offset is not combined with
/// cursor bounds. Apply sorting, then offset or cursor bounds, then limit.
/// `limit` selects the first entries of that ordered interval; reverse `sort`
/// for reverse traversal rather than introducing `first`/`last` options.
/// Programs must define stable ordering, tie-breaking, and how missing or deleted
/// cursor IDs are handled. ID-based pagination avoids positional drift when
/// entries are inserted or removed, but does not itself provide snapshot isolation.
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
/// are independent. `Default` and an empty builder leave all capabilities
/// [`Unknown`](OptionSupport::Unknown). Output-format support is not configured
/// here. A host may obtain these declarations from
/// module metadata or other knowledge of the program; this crate performs no
/// discovery. Concrete forwarding and fallback policies belong to the executor.
///
/// ```
/// use asimov_patterns::{ListerCapabilities, OptionSupport};
///
/// let capabilities = ListerCapabilities::builder()
///     .sort(OptionSupport::Unsupported)
///     .offset(OptionSupport::Supported)
///     .limit(OptionSupport::Unsupported)
///     .after(OptionSupport::Supported)
///     .build();
/// assert_eq!(capabilities.sort, OptionSupport::Unsupported);
/// assert_eq!(capabilities.offset, OptionSupport::Supported);
/// assert_eq!(capabilities.limit, OptionSupport::Unsupported);
/// assert_eq!(capabilities.after, OptionSupport::Supported);
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

    /// Native `--before` support for an exclusive entry-ID cursor bound.
    #[builder(default)]
    pub before: OptionSupport,

    /// Native `--after` support for an exclusive entry-ID cursor bound.
    #[builder(default)]
    pub after: OptionSupport,

    /// Native `--limit` support. A host can omit an unsupported native limit
    /// while still enforcing the requested result count locally.
    #[builder(default)]
    pub limit: OptionSupport,
}

/// Output-format and pagination requests for a [`Lister`].
///
/// `Default` leaves all optional fields unset and `other` empty: no caller
/// limit, no skipped entries or cursor bounds, and the program's default order
/// and output format. The collection URL is supplied separately by the runner.
///
/// These fields express requested behavior, not program capabilities; supply
/// known native support separately using [`ListerCapabilities`].
/// Sorting and all pagination options have explicit native capabilities, and
/// the portable sort-expression grammar is still unresolved. Use only keys
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
///     .offset(20)
///     .limit(100)
///     .output("jsonl")
///     .build();
///
/// let cursor_page = ListerOptions::builder()
///     .after("urn:example:entry:123")
///     .limit(25)
///     .build();
/// assert_eq!(cursor_page.after.as_deref(), Some("urn:example:entry:123"));
/// assert!(cursor_page.offset.is_none());
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
    /// native support. Do not combine an offset, even `Some(0)`, with `before`
    /// or `after`; these are alternative pagination modes.
    pub offset: Option<usize>,

    /// Exclusive upper cursor bound, passed as `--before=URI`.
    ///
    /// The URI identifies an entry by its JSON-LD `@id`; it is an absolute URI
    /// string, not a JSON object or an opaque encoded cursor. Select entries
    /// before that entry in the chosen sort order. May be combined with `after`
    /// to bound an interval, but not with numeric `offset`. `None` omits the bound.
    /// The options value stores this string without validation or normalization.
    pub before: Option<String>,

    /// Exclusive lower cursor bound, passed as `--after=URI`.
    ///
    /// The URI identifies an entry by its JSON-LD `@id`. Select entries after
    /// that entry in the chosen sort order. May be combined with `before`, but
    /// not with numeric `offset`. `None` omits the bound. As with `before`, the
    /// options value stores the absolute URI string without validating it.
    pub after: Option<String>,

    /// Requested listing limit (`--limit=COUNT`, or `-n`, when forwarded natively).
    ///
    /// `None` imposes no caller-requested limit; `Some(0)` requests no entries.
    /// [`ListerCapabilities::limit`] describes native support. A host can enforce
    /// the request even when the native flag is unsupported, and independently
    /// cap output to protect against bugs in programs that do accept the flag.
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
