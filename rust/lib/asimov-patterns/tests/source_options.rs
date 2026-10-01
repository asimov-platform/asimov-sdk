// This is free and unencumbered software released into the public domain.

extern crate alloc;

use alloc::string::String;
use asimov_patterns::{
    CachingOptions, FetcherOptions, FilteringOptions, ListerOptions, TimingOptions,
};
use core::time::Duration;

#[test]
fn builders_preserve_unset_defaults() {
    assert_eq!(CachingOptions::builder().build(), CachingOptions::default());
    assert_eq!(
        FilteringOptions::builder().build(),
        FilteringOptions::default()
    );
    assert_eq!(TimingOptions::builder().build(), TimingOptions::default());
    assert_eq!(FetcherOptions::builder().build(), FetcherOptions::default());
    assert_eq!(
        ListerOptions::<String>::builder().build(),
        ListerOptions::default()
    );
    assert_eq!(ListerOptions::<String>::default().offset, None);

    let cache = CachingOptions::builder()
        .max_age(Duration::from_secs(3600))
        .build();
    assert_eq!(cache.max_age, Some(Duration::from_secs(3600)));
    let timing = TimingOptions::builder().deadline(Duration::ZERO).build();
    assert_eq!(timing.deadline, Some(Duration::ZERO));

    let filtering = FilteringOptions::builder()
        .jev("The name is Ukrainian")
        .jq("select(.name)")
        .build();
    assert_eq!(filtering.jev.as_deref(), Some("The name is Ukrainian"));
    assert_eq!(filtering.jq.as_deref(), Some("select(.name)"));
}

#[cfg(feature = "clap")]
mod cli {
    use super::*;
    use alloc::{
        format,
        string::{String, ToString},
        vec::Vec,
    };
    use asimov_patterns::OutputFormat;
    use clap::{Args, Command, CommandFactory, Parser, error::ErrorKind};

    #[derive(Debug, Parser)]
    struct FetchCommand {
        #[command(flatten)]
        options: FetcherOptions,
        #[command(flatten)]
        filtering: FilteringOptions,
        #[command(flatten)]
        cache: CachingOptions,
        #[command(flatten)]
        timing: TimingOptions,
        #[arg(short = 'M', long)]
        module: Option<String>,
        urls: Vec<String>,
    }

    #[derive(Debug, Parser)]
    struct ListCommand {
        #[command(flatten)]
        options: ListerOptions,
        #[command(flatten)]
        filtering: FilteringOptions,
        #[command(flatten)]
        cache: CachingOptions,
        #[command(flatten)]
        timing: TimingOptions,
        #[arg(short = 'M', long)]
        module: Option<String>,
        urls: Vec<String>,
    }

    #[test]
    fn argument_groups_do_not_supply_rust_docs_as_command_descriptions() {
        for command in [
            CachingOptions::augment_args(Command::new("cache")),
            TimingOptions::augment_args(Command::new("timing")),
            FilteringOptions::augment_args(Command::new("filter")),
            FetcherOptions::augment_args(Command::new("fetch")),
            ListerOptions::<String>::augment_args(Command::new("list")),
        ] {
            assert!(command.get_about().is_none(), "{}", command.get_name());
            assert!(command.get_long_about().is_none(), "{}", command.get_name());
        }

        #[derive(Parser)]
        #[command(
            about = "List resources",
            long_about = "List resources from collection URLs."
        )]
        struct HostCommand {
            #[command(flatten)]
            args: ListCommand,
        }

        let mut command = HostCommand::command();
        assert!(
            command
                .render_help()
                .to_string()
                .starts_with("List resources\n")
        );
        assert!(
            command
                .render_long_help()
                .to_string()
                .starts_with("List resources from collection URLs.\n")
        );
    }

    #[test]
    fn short_and_extended_help_are_cli_facing() {
        for command in [FetchCommand::command(), ListCommand::command()] {
            for flag in ["-h", "--help"] {
                let error = command
                    .clone()
                    .try_get_matches_from(["test", flag])
                    .unwrap_err();
                assert_eq!(error.kind(), ErrorKind::DisplayHelp);
                let help = error.to_string();
                let help = help.split_whitespace().collect::<Vec<_>>().join(" ");

                for option in [
                    "--output <FORMAT>",
                    "--jev <NOUL>",
                    "--jq <EXPR>",
                    "--max-age <DURATION>",
                    "--deadline <DURATION>",
                ] {
                    assert!(help.contains(option), "missing {option} in {flag}: {help}");
                }
                for internal in [
                    "asimov-runner",
                    "Self::",
                    "crate::",
                    "[`",
                    "Option<",
                    "Some(",
                    "`None`",
                    "builder",
                    "Applied by the host",
                    "This type stores",
                    "FetcherOptions",
                    "ListerOptions",
                    "FilteringOptions",
                    "CachingOptions",
                    "TimingOptions",
                ] {
                    assert!(
                        !help.contains(internal),
                        "leaked {internal} in {flag}: {help}"
                    );
                }

                if flag == "--help" {
                    for example in ["positive duration", "relative duration", "select(.name)"] {
                        assert!(help.contains(example), "missing {example}: {help}");
                    }
                    assert!(help.contains("The name is Ukrainian"), "{help}");
                    assert!(
                        help.contains("an assertion evaluated as true or false"),
                        "{help}"
                    );
                    assert!(help.contains("before --jq"), "{help}");
                    assert!(help.contains("after Jev filtering"), "{help}");
                }
            }
        }
    }

    #[test]
    fn flattened_commands_preserve_cli_defaults() {
        FetchCommand::command().debug_assert();
        ListCommand::command().debug_assert();

        let fetch = FetchCommand::try_parse_from(["fetch"]).unwrap();
        assert_eq!(fetch.options, FetcherOptions::default());
        assert_eq!(fetch.filtering, FilteringOptions::default());
        assert_eq!(fetch.cache, CachingOptions::default());
        assert_eq!(fetch.timing, TimingOptions::default());
        assert_eq!(fetch.cache.max_age_option(), None);
        assert_eq!(fetch.timing.deadline_option(), None);

        let list = ListCommand::try_parse_from(["list"]).unwrap();
        assert_eq!(
            list.options,
            ListerOptions {
                offset: Some(0),
                ..Default::default()
            }
        );
        assert_eq!(list.cache, CachingOptions::default());
        assert_eq!(list.filtering, FilteringOptions::default());
        assert_eq!(list.timing, TimingOptions::default());
    }

    #[test]
    fn fetch_accepts_both_filters_and_forwards_durations() {
        let args = FetchCommand::try_parse_from([
            "fetch",
            "-M",
            "example",
            "-o",
            "jsonl",
            "--jev",
            "The name is Ukrainian",
            "--jq",
            ".name",
            "--max-age",
            "1h",
            "--deadline",
            "30s",
            "https://example.com/one",
            "https://example.com/two",
        ])
        .unwrap();
        assert_eq!(args.module.as_deref(), Some("example"));
        assert_eq!(args.urls.len(), 2);
        assert_eq!(args.options.output.as_deref(), Some("jsonl"));
        assert_eq!(args.filtering.jev.as_deref(), Some("The name is Ukrainian"));
        assert_eq!(args.filtering.jq.as_deref(), Some(".name"));
        assert!(args.options.other.is_empty());
        assert_eq!(args.cache.max_age, Some(Duration::from_secs(3600)));
        assert_eq!(args.timing.deadline, Some(Duration::from_secs(30)));
        assert_eq!(args.cache.max_age_option().as_deref(), Some("--max-age=1h"));
        assert_eq!(
            args.timing.deadline_option().as_deref(),
            Some("--deadline=30s")
        );
    }

    #[test]
    fn list_accepts_sort_aliases_pagination_and_both_filters() {
        for sort in ["--sort", "--sort-by", "--order", "--order-by"] {
            let args = ListCommand::try_parse_from([
                "list",
                sort,
                "-name",
                "--offset",
                "20",
                "-n",
                "100",
                "--output",
                "jsonl",
                "--jev",
                "The name is Ukrainian",
                "--jq",
                "select(.name)",
                "--max-age",
                "7d",
                "--deadline",
                "0s",
                "https://example.com/collection",
            ])
            .unwrap();
            assert_eq!(args.options.sort.unwrap().to_string(), "-name");
            assert_eq!(args.options.offset, Some(20));
            assert_eq!(args.options.limit, Some(100));
            assert_eq!(args.options.output, Some(OutputFormat::Jsonl));
            assert_eq!(args.filtering.jev.as_deref(), Some("The name is Ukrainian"));
            assert_eq!(args.filtering.jq.as_deref(), Some("select(.name)"));
            assert_eq!(args.cache.max_age, Some(Duration::from_secs(7 * 86400)));
            assert_eq!(args.timing.deadline, Some(Duration::ZERO));
            assert_eq!(
                args.timing.deadline_option().as_deref(),
                Some("--deadline=0s")
            );
        }
    }

    #[test]
    fn duration_arguments_round_trip_compound_and_subsecond_values() {
        for duration in ["1m 30s", "0.5s", "1ns", "2h 15m 3s 4ms"] {
            let first = FetchCommand::try_parse_from([
                "fetch",
                "--max-age",
                duration,
                "--deadline",
                duration,
            ])
            .unwrap();
            let second = FetchCommand::try_parse_from([
                String::from("fetch"),
                first.cache.max_age_option().unwrap(),
                first.timing.deadline_option().unwrap(),
            ])
            .unwrap();
            assert_eq!(first.cache, second.cache);
            assert_eq!(first.timing, second.timing);
        }
    }

    #[test]
    fn rejects_zero_max_age_and_invalid_durations() {
        for (option, values) in [
            ("--max-age", ["0s", "0", "-1h", "nope"]),
            ("--deadline", ["nope", "", "-1s", "1fortnight"]),
        ] {
            for value in values {
                let arg = format!("{option}={value}");
                let error = FetchCommand::try_parse_from(["fetch", &arg]).unwrap_err();
                assert_eq!(error.kind(), ErrorKind::ValueValidation, "{arg}");
            }
        }
        let error = FetchCommand::try_parse_from(["fetch", "--max-age=0s"]).unwrap_err();
        assert!(error.to_string().contains("max age must be positive"));
        assert!(FetchCommand::try_parse_from(["fetch", "--wait"]).is_err());
    }
}
