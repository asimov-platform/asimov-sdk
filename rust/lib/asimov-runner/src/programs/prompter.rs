// This is free and unencumbered software released into the public domain.

//! LLM inference through an external prompter program.
//!
//! [`Prompt`], [`PromptMessage`], and [`PromptRole`] are re-exported here for
//! constructing the input passed to [`Prompter`].

use crate::{CommandExt, Executor, ExecutorError, Input, TextOutput};
use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{
    ffi::OsStr,
    io::{Cursor, Read},
    process::Stdio,
};

pub use asimov_patterns::PrompterOptions;
pub use asimov_prompt::{Prompt, PromptMessage, PromptRole};

/// The complete UTF-8 stdout of a successful [`Prompter`], or an execution error.
///
/// Whitespace and trailing newlines are preserved. Invalid UTF-8 is reported as
/// [`ExecutorError::UnexpectedOther`]. Non-captured output returns an empty string.
pub type PrompterResult = std::result::Result<String, ExecutorError>;

/// An external [prompter] that provides LLM inference for a [`Prompt`].
///
/// Each execution formats the stored prompt with `Display` and feeds it to stdin
/// concurrently with routing stdout and draining stderr. Prompt writes are
/// awaited as part of execution, with failures reported in the result.
///
/// Captured responses are buffered and decoded as UTF-8 without trimming or
/// further parsing. Forwarded responses are copied as raw bytes to the supplied
/// writer; ignored/inherited responses are not decoded. Format and model options
/// do not change prompt serialization or captured-response decoding.
/// The specification's default `text` format does not define chat role prefixes;
/// the selected program must understand the stored prompt's display convention.
///
/// [prompter]: https://asimov-specs.github.io/program-patterns/#prompter
#[allow(unused)]
#[derive(Debug)]
pub struct Prompter {
    executor: Executor,
    options: PrompterOptions,
    input: Prompt,
    output: TextOutput,
}

impl Prompter {
    /// Configures a prompter without starting it.
    ///
    /// Adds any configured `--input=<format>`, `--output=<format>`, and
    /// `--model=<model>` arguments, followed by `options.other`. Stdin and stderr
    /// are piped; `output` selects stdout routing.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: Prompt,
        output: TextOutput,
        options: PrompterOptions,
    ) -> Self {
        let mut executor = Executor::new(program);

        executor
            .command()
            .option("input", options.input.as_ref())
            .option("output", options.output.as_ref())
            .option("model", options.model.as_ref())
            .args(&options.other)
            .stdin(Stdio::piped())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    /// Sends the stored prompt to a new child and returns its complete text response.
    ///
    /// Repeated calls send the same prompt again. Input delivery, output routing,
    /// and process completion are awaited together. Cancelling the future drops
    /// the child handle and the prompt feed; there is no detached writing task.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] for spawning, input delivery, output forwarding,
    /// waiting, unsuccessful exit, or invalid UTF-8 in captured stdout. Error
    /// precedence follows [`crate::ExecutionCompletion::into_result`].
    pub async fn execute(&mut self) -> PrompterResult {
        let mut input =
            Input::AsyncRead(Box::new(Cursor::new(self.input.to_string().into_bytes())));
        let mut stdout = self
            .executor
            .execute_with_io(&mut input, &mut self.output)
            .await?;
        let mut result = String::new();
        stdout.read_to_string(&mut result)?;

        Ok(result)
    }
}

impl asimov_patterns::Prompter<String> for Prompter {}

#[async_trait]
impl asimov_patterns::Execute<String> for Prompter {
    type Error = ExecutorError;

    async fn execute(&mut self) -> PrompterResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    //use asimov_patterns::Execute;

    #[tokio::test]
    async fn test_execute() {
        let mut prompter = Prompter::new(
            "cat",
            Prompt::builder()
                .messages(vec![PromptMessage(
                    PromptRole::User,
                    "Hello, world!".into(),
                )])
                .build(),
            TextOutput::Captured,
            PrompterOptions::default(),
        );
        let result = prompter.execute().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), String::from("user: Hello, world!\n"));
    }
}
