// This is free and unencumbered software released into the public domain.

use super::ListerOptions;
use crate::OutputFormat;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use clap::{ArgMatches, Args, Command, Error, FromArgMatches, Id};
use clientele::options::sort::{SortKey, SortKeys};
use core::{fmt::Display, str::FromStr};

const HELP_SORT: &str =
    "Sort resources by the specified keys (prefix a key with `-` for descending order)";
const HELP_OFFSET: &str = "The index offset of the first output";
const HELP_LIMIT: &str = "The maximum count of outputs [default: none]";
const HELP_OUTPUT: &str = "The output format: jsonl, url, or an application-specific format.";

/// Private Clap representation of the CLI-exposed [`ListerOptions`] fields.
///
/// Keeping the derive here confines parsing bounds (`FromStr`, `Send`, `Sync`,
/// `'static`, and displayable errors) to the Clap integration. Programmatic
/// construction of `ListerOptions<T, F>` only requires `T: Clone`; it does not
/// require parsers or thread-safe types, even with the `clap` feature enabled.
///
/// The manual implementations below connect the two structs:
///
/// - [`Args`] on `ListerOptions` delegates `group_id`, `augment_args`, and
///   `augment_args_for_update` to this derived implementation. A containing
///   command can therefore flatten `ListerOptions` directly.
/// - [`FromArgMatches::from_arg_matches_mut`] parses into this struct, then
///   moves `sort`, `offset`, `limit`, and `output` into `ListerOptions`.
///   The programmatic-only `other`, `before`, and `after` fields get defaults.
/// - [`FromArgMatches::update_from_arg_matches_mut`] first copies or clones
///   the four CLI fields from existing options into a temporary `ListerArgs`,
///   delegates the update to Clap, then moves the updated fields back.
///   The programmatic-only fields retain their existing values.
/// - The immutable-match methods clone the matches and call their mutable
///   counterparts, preserving the caller's `ArgMatches`.
///
/// There is no automatic synchronization between the structs. When adding a
/// CLI field, update both declarations and the construction and update mappings
/// in `FromArgMatches` below, along with the parsing/update tests.
///
/// Explicit `help` and `long_help` attributes provide CLI-facing text. Clearing
/// `about` and `long_about` prevents these Rust docs from becoming the containing
/// command's description; that command supplies its own description.
#[derive(Args)]
#[command(about = None, long_about = None)]
#[group(id = "ListerOptions")]
struct ListerArgs<T, F>
where
    T: Clone + FromStr + Send + Sync + 'static,
    T::Err: Display,
    F: Clone + FromStr + Send + Sync + 'static,
    F::Err: Display,
{
    #[arg(
        long,
        aliases = ["sort-by", "order", "order-by"],
        value_name = "[+|-]KEY,...",
        value_parser = parse_sort::<T>,
        help = HELP_SORT,
        long_help = HELP_SORT,
        allow_hyphen_values = true
    )]
    sort: Option<SortKeys<T>>,

    #[arg(
        long,
        value_name = "INDEX",
        default_value = "0",
        help = HELP_OFFSET,
        long_help = HELP_OFFSET
    )]
    offset: Option<usize>,

    #[arg(long, short = 'n', value_name = "COUNT", help = HELP_LIMIT, long_help = HELP_LIMIT)]
    limit: Option<usize>,

    #[arg(
        long,
        short = 'o',
        value_name = "FORMAT",
        value_parser = parse_output::<F>,
        help = "The output format",
        long_help = HELP_OUTPUT
    )]
    output: Option<OutputFormat<F>>,
}

fn parse_sort<T: Clone + FromStr>(value: &str) -> Result<SortKeys<T>, String>
where
    T::Err: Display,
{
    // Reuse Clientele's grammar and validation, then parse each bare key as T.
    let keys = value.parse::<SortKeys>()?;
    keys.keys()
        .iter()
        .map(|key| {
            key.key()
                .parse::<T>()
                .map(|value| SortKey::new(value, key.descending()))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(SortKeys::from)
}

fn parse_output<F: FromStr>(value: &str) -> Result<OutputFormat<F>, String>
where
    F::Err: Display,
{
    value.parse().map_err(|error: F::Err| error.to_string())
}

impl<T, F> Args for ListerOptions<T, F>
where
    T: Clone + FromStr + Send + Sync + 'static,
    T::Err: Display,
    F: Clone + FromStr + Send + Sync + 'static,
    F::Err: Display,
{
    fn group_id() -> Option<Id> {
        ListerArgs::<T, F>::group_id()
    }

    fn augment_args(command: Command) -> Command {
        ListerArgs::<T, F>::augment_args(command)
    }

    fn augment_args_for_update(command: Command) -> Command {
        ListerArgs::<T, F>::augment_args_for_update(command)
    }
}

impl<T, F> FromArgMatches for ListerOptions<T, F>
where
    T: Clone + FromStr + Send + Sync + 'static,
    T::Err: Display,
    F: Clone + FromStr + Send + Sync + 'static,
    F::Err: Display,
{
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        Self::from_arg_matches_mut(&mut matches.clone())
    }

    fn from_arg_matches_mut(matches: &mut ArgMatches) -> Result<Self, Error> {
        let args = ListerArgs::<T, F>::from_arg_matches_mut(matches)?;
        Ok(Self {
            sort: args.sort,
            offset: args.offset,
            limit: args.limit,
            output: args.output,
            ..Self::default()
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        self.update_from_arg_matches_mut(&mut matches.clone())
    }

    fn update_from_arg_matches_mut(&mut self, matches: &mut ArgMatches) -> Result<(), Error> {
        let mut args = ListerArgs {
            sort: self.sort.clone(),
            offset: self.offset,
            limit: self.limit,
            output: self.output.clone(),
        };
        args.update_from_arg_matches_mut(matches)?;
        self.sort = args.sort;
        self.offset = args.offset;
        self.limit = args.limit;
        self.output = args.output;
        Ok(())
    }
}
