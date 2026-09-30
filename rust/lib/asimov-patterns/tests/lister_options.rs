// This is free and unencumbered software released into the public domain.

extern crate alloc;

use alloc::string::{String, ToString};
use asimov_patterns::{ListerOptions, OutputFormat};
use clientele::options::sort::{SortKey, SortKeys};
use core::{fmt, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq)]
enum MyProps {
    FirstName,
    LastName,
}

impl FromStr for MyProps {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "first-name" => Ok(Self::FirstName),
            "last-name" => Ok(Self::LastName),
            _ => Err("expected first-name or last-name"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MyFormats {
    Csv,
}

impl FromStr for MyFormats {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "csv" => Ok(Self::Csv),
            _ => Err("expected csv"),
        }
    }
}

impl fmt::Display for MyFormats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Csv => f.write_str("csv"),
        }
    }
}

#[test]
fn standard_and_custom_formats_round_trip() {
    for (name, expected) in [
        ("jsonl", OutputFormat::Jsonl),
        ("url", OutputFormat::Url),
        ("turtle", OutputFormat::Other(String::from("turtle"))),
    ] {
        let format: OutputFormat = name.parse().unwrap();
        assert_eq!(format, expected);
        assert_eq!(format.to_string(), name);
        assert_eq!(format.as_str(), name);
        assert_eq!(format.as_ref(), name);
        assert_eq!(OutputFormat::from(name), expected);
        assert_eq!(OutputFormat::from(String::from(name)), expected);
    }

    for (name, expected) in [
        ("jsonl", OutputFormat::Jsonl),
        ("url", OutputFormat::Url),
        ("csv", OutputFormat::Other(MyFormats::Csv)),
    ] {
        let format: OutputFormat<MyFormats> = name.parse().unwrap();
        assert_eq!(format, expected);
        assert_eq!(format.to_string(), name);
    }
    assert_eq!(
        "turtle".parse::<OutputFormat<MyFormats>>(),
        Err("expected csv")
    );
    assert_eq!(OutputFormat::<MyFormats>::default(), OutputFormat::Jsonl);
}

#[test]
fn enum_sort_keys_and_output_formats_are_independent() {
    let sort = SortKeys::new(&[
        SortKey::new(MyProps::LastName, true),
        SortKey::new(MyProps::FirstName, false),
    ]);
    let options = ListerOptions::<MyProps>::builder()
        .sort(sort.clone())
        .output("turtle")
        .other("--custom")
        .maybe_other(Some("literal argument"))
        .build();
    assert_eq!(options.sort, Some(sort));
    assert_eq!(
        options.output,
        Some(OutputFormat::Other(String::from("turtle")))
    );
    assert_eq!(options.other, ["--custom", "literal argument"]);

    let options = ListerOptions::<MyProps, MyFormats>::builder()
        .output(OutputFormat::Other(MyFormats::Csv))
        .build();
    assert_eq!(options.output, Some(OutputFormat::Other(MyFormats::Csv)));
    assert_eq!(
        ListerOptions::<MyProps, MyFormats>::builder().build(),
        ListerOptions::default()
    );

    // Storage and builders do not require parsing or defaults for key types,
    // even when the crate's Clap feature is enabled.
    #[derive(Clone)]
    struct Key;
    struct Format;
    let options = ListerOptions::<Key, Format>::builder().build();
    assert!(options.sort.is_none());
    assert!(options.output.is_none());
    assert!(ListerOptions::<Key, Format>::default().output.is_none());
}

#[cfg(feature = "clap")]
mod cli {
    use super::*;
    use clap::{CommandFactory, FromArgMatches, Parser, error::ErrorKind};

    #[derive(Debug, Parser)]
    struct TypedList {
        #[command(flatten)]
        options: ListerOptions<MyProps, MyFormats>,
    }

    #[test]
    fn parses_enum_keys_and_independent_format_enum() {
        TypedList::command().debug_assert();
        for alias in ["--sort", "--sort-by", "--order", "--order-by"] {
            let args =
                TypedList::try_parse_from(["list", alias, "-last-name,+first-name", "-o", "csv"])
                    .unwrap();
            assert_eq!(
                args.options.sort,
                Some(SortKeys::new(&[
                    SortKey::new(MyProps::LastName, true),
                    SortKey::new(MyProps::FirstName, false),
                ]))
            );
            assert_eq!(
                args.options.output,
                Some(OutputFormat::Other(MyFormats::Csv))
            );
        }
        for (name, expected) in [("jsonl", OutputFormat::Jsonl), ("url", OutputFormat::Url)] {
            let args = TypedList::try_parse_from(["list", "--output", name]).unwrap();
            assert_eq!(args.options.output, Some(expected));
        }
        let args = TypedList::try_parse_from(["list"]).unwrap();
        assert!(args.options.output.is_none());
        assert!(args.options.sort.is_none());
        assert_eq!(args.options.offset, Some(0));
    }

    #[test]
    fn rejects_invalid_typed_values_and_sort_grammar() {
        for value in [
            "unknown",
            "first-name,unknown",
            "",
            ",first-name",
            "first-name,",
            "+",
            "--last-name",
        ] {
            let arg = alloc::format!("--sort={value}");
            let error = TypedList::try_parse_from(["list", &arg]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{arg}");
        }
        let error = TypedList::try_parse_from(["list", "--sort=unknown"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("expected first-name or last-name")
        );
        let error = TypedList::try_parse_from(["list", "--output=turtle"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        assert!(error.to_string().contains("expected csv"));
    }

    #[test]
    fn updating_arguments_preserves_programmatic_fields() {
        let mut args = TypedList {
            options: ListerOptions::builder()
                .other("--custom")
                .before("urn:entry:10")
                .after("urn:entry:1")
                .output(OutputFormat::Other(MyFormats::Csv))
                .build(),
        };
        args.try_update_from(["list", "--sort=last-name", "--output=url"])
            .unwrap();
        assert_eq!(args.options.output, Some(OutputFormat::Url));
        assert_eq!(
            args.options.sort,
            Some(SortKeys::new(&[SortKey::new(MyProps::LastName, false)]))
        );
        assert_eq!(args.options.other, ["--custom"]);
        assert_eq!(args.options.before.as_deref(), Some("urn:entry:10"));
        assert_eq!(args.options.after.as_deref(), Some("urn:entry:1"));

        let matches = TypedList::command_for_update()
            .try_get_matches_from(["list", "--limit=5"])
            .unwrap();
        args.options.update_from_arg_matches(&matches).unwrap();
        assert_eq!(args.options.limit, Some(5));
        assert_eq!(args.options.output, Some(OutputFormat::Url));
        assert_eq!(args.options.other, ["--custom"]);
    }
}
