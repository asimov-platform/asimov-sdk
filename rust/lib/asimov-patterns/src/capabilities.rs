// This is free and unencumbered software released into the public domain.

//! Caller-supplied metadata about a program's native option support.
//!
//! Capabilities describe the program, while options describe the requested
//! operation. Discovery and manifest parsing belong to the caller. These values
//! neither probe a program nor select an emulation strategy.

/// Knowledge of a program's native support for an optional command-line option.
///
/// This is a declaration, not a guarantee of correct behavior or support for
/// every possible option value. Hosts document how unknown and unsupported
/// capabilities affect execution. Required options do not need capability flags.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OptionSupport {
    /// No support information was supplied; this does not mean unsupported.
    #[default]
    Unknown,
    /// The program is declared to support this option natively.
    Supported,
    /// The program is declared not to support this option natively. A host must
    /// emulate a requested operation or reject it rather than silently omit it.
    Unsupported,
}
