// This is free and unencumbered software released into the public domain.

use super::*;
use crate::{BatchOptions, Pipeline, Writer, WriterOptions};
use core::time::Duration;

#[derive(Clone, core::fmt::Debug)]
enum Property {
    FirstName,
    LastName,
    Rank,
}

impl fmt::Display for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FirstName => "first-name",
            Self::LastName => "last-name",
            Self::Rank => "rank",
        })
    }
}

// Neither enum implements FromStr or AsRef<str>; custom formats need not Clone.
#[derive(core::fmt::Debug)]
enum Format {
    Csv,
    JsonlAlias,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Csv => "csv",
            Self::JsonlAlias => "jsonl",
        })
    }
}

#[test]
fn typed_names_survive_capability_changes_and_batching() {
    let batching = BatchOptions::new(1, 4096, Duration::from_secs(1)).unwrap();
    let mut lister = Lister::new(
        "asimov-test-lister",
        "example:collection",
        GraphOutput::Captured,
        ListerOptions::<Property, Format>::builder()
            .sort(SortKeys::new(&[
                SortKey::new(Property::LastName, true),
                SortKey::new(Property::FirstName, false),
            ]))
            .offset(2)
            .limit(3)
            .output(OutputFormat::Other(Format::Csv))
            .other("literal argument")
            .build(),
    )
    .with_batching(batching);

    for support in [OptionSupport::Unsupported, OptionSupport::Supported] {
        lister = lister.with_capabilities(ListerCapabilities::builder().limit(support).build());
        assert_eq!(lister.executor.batch_options(), batching);
        let arguments: Vec<_> = lister.executor.command().as_std().get_args().collect();
        let mut expected = alloc::vec!["--sort=-last-name,first-name", "--offset=2"];
        if support == OptionSupport::Supported {
            expected.push("--limit=3");
        }
        expected.extend(["--output=csv", "literal argument", "example:collection"]);
        assert_eq!(arguments, expected);
    }
}

fn cursor_lister(limit: Option<usize>) -> Lister<Property, Format> {
    Lister::new(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/lister-cursors.sh"
        ),
        "example:collection",
        GraphOutput::Captured,
        ListerOptions::builder()
            .sort(SortKeys::new(&[SortKey::new(Property::Rank, false)]))
            .after("urn:item:alpha")
            .before("urn:item:omega")
            .maybe_limit(limit)
            .build(),
    )
    .with_batching(BatchOptions::new(1, 4096, Duration::from_secs(1)).unwrap())
    .with_capabilities(
        ListerCapabilities::builder()
            .limit(OptionSupport::Unsupported)
            .build(),
    )
}

#[tokio::test]
async fn typed_listers_execute_via_pattern_traits_and_pipelines() {
    async fn execute(
        lister: &mut impl asimov_patterns::Lister<ListerStream, Error = ExecutorError>,
    ) -> Vec<u8> {
        collect(lister.execute().await.unwrap()).await
    }
    async fn collect<E: fmt::Debug>(mut stream: crate::BatchStream<E>) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Some(batch) = stream.next().await {
            let batch = batch.unwrap();
            assert_eq!(batch.lines().len(), 1);
            for line in batch.lines() {
                bytes.extend_from_slice(line);
            }
        }
        bytes
    }

    for (limit, expected) in [
        (
            None,
            b"{\"@id\":\"urn:item:zeta\"}\n{\"@id\":\"urn:item:beta\"}\n".as_slice(),
        ),
        (Some(1), b"{\"@id\":\"urn:item:zeta\"}\n".as_slice()),
        (Some(0), b"".as_slice()),
    ] {
        assert_eq!(execute(&mut cursor_lister(limit)).await, expected);
        let stream = Pipeline::new(cursor_lister(limit)).execute().await.unwrap();
        assert_eq!(collect(stream).await, expected);

        let writer = Writer::new(
            "/bin/sh",
            Input::Ignored,
            crate::AnyOutput::Captured,
            WriterOptions::builder().other("-c").other("cat").build(),
        );
        let bytes = Pipeline::new(cursor_lister(limit))
            .pipe(writer)
            .execute()
            .await
            .unwrap()
            .into_inner();
        assert_eq!(bytes, expected);
    }
}

#[tokio::test]
async fn typed_validation_precedes_spawning_and_zero_limit() {
    let make = |output| {
        Lister::new(
            "/this-lister-does-not-exist",
            "example:",
            GraphOutput::Captured,
            ListerOptions::<Property, Format>::builder()
                .limit(0)
                .output(output)
                .build(),
        )
    };
    for output in [OutputFormat::Url, OutputFormat::Other(Format::Csv)] {
        assert!(matches!(
            Pipeline::new(make(output)).execute().await,
            Err(crate::PipelineError { stage: 0, error: ExecutorError::UnexpectedOther(ref error), .. })
                if error.kind() == std::io::ErrorKind::InvalidInput
        ));
    }
    for output in [OutputFormat::Jsonl, OutputFormat::Other(Format::JsonlAlias)] {
        let mut stream = Pipeline::new(make(output)).execute().await.unwrap();
        assert!(stream.next().await.is_none());
    }

    let mut unsupported = cursor_lister(Some(0)).with_capabilities(
        ListerCapabilities::builder()
            .sort(OptionSupport::Unsupported)
            .build(),
    );
    assert!(matches!(
        unsupported.execute().await,
        Err(ExecutorError::UnsupportedOption("--sort"))
    ));
    assert!(matches!(
        Pipeline::new(unsupported).execute().await,
        Err(crate::PipelineError {
            error: ExecutorError::UnsupportedOption("--sort"),
            ..
        })
    ));

    let mut invalid = Lister::<Property, Format>::new(
        "/this-lister-does-not-exist",
        "example:",
        GraphOutput::Captured,
        ListerOptions::builder()
            .offset(0)
            .after("urn:item:alpha")
            .limit(0)
            .build(),
    );
    assert!(
        matches!(invalid.execute().await, Err(ExecutorError::UnexpectedOther(ref error))
        if error.kind() == std::io::ErrorKind::InvalidInput)
    );
}
