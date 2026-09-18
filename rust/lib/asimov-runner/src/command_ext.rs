// This is free and unencumbered software released into the public domain.

//! Fluent construction of optional long-form command-line arguments.

use crate::Command;
use alloc::format;
use core::fmt::Display;

/// Additional argument-building methods for Tokio [`Command`].
///
/// Import this trait to chain optional named arguments with the command's
/// existing `arg`, `args`, and standard-stream configuration methods.
///
/// ```
/// use asimov_runner::{Command, CommandExt};
///
/// let mut command = Command::new("asimov-example-lister");
/// let before = Some(String::from("urn:example:entry:123"));
/// command
///     .option("sort", Some("name"))
///     .option("offset", None::<usize>)
///     .option("before", before.as_ref())
///     .option("limit", Some(25))
///     .arg("https://example.com/collection");
///
/// let args: Vec<_> = command.as_std().get_args().collect();
/// assert_eq!(args, [
///     "--sort=name",
///     "--before=urn:example:entry:123",
///     "--limit=25",
///     "https://example.com/collection",
/// ]);
/// ```
pub trait CommandExt {
    /// Appends `--name=value` for `Some(value)` and nothing for `None`.
    ///
    /// `name` is the long option name without leading dashes. Values use
    /// [`Display`], accepting numbers, strings, and domain types such as sort
    /// keys. Use `as_ref()` to borrow a non-`Copy` optional value. `Some(0)`,
    /// `Some(false)`, and `Some("")` are explicit values and are not omitted.
    ///
    /// Each present option is one literal argument, preserving invocation order,
    /// spaces, and punctuation without shell escaping or expansion. Names and
    /// values are not validated. Capability decisions remain with the caller:
    /// use [`Option::filter`] to conditionally omit an otherwise present option.
    fn option(&mut self, name: &str, value: Option<impl Display>) -> &mut Self;
}

impl CommandExt for Command {
    fn option(&mut self, name: &str, value: Option<impl Display>) -> &mut Self {
        if let Some(value) = value {
            self.arg(format!("--{name}={value}"));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn preserves_explicit_values_argument_boundaries_and_order() {
        let mut command = Command::new("asimov-test-program");
        command
            .arg("first")
            .option("missing", None::<&str>)
            .option("offset", Some(0))
            .option("enabled", Some(false))
            .option("empty", Some(""))
            .option("literal", Some("café a=b; '$HOME'\nnext"))
            .option("repeat", Some(1))
            .option("repeat", Some(2))
            .arg("last");
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(
            args,
            [
                "first",
                "--offset=0",
                "--enabled=false",
                "--empty=",
                "--literal=café a=b; '$HOME'\nnext",
                "--repeat=1",
                "--repeat=2",
                "last",
            ]
        );
    }
}
