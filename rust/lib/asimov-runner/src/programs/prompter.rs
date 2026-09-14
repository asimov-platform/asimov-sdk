// This is free and unencumbered software released into the public domain.

//! LLM inference through an external prompter program.
//!
//! [`Prompt`], [`PromptMessage`], and [`PromptRole`] are re-exported here for
//! constructing the input passed to [`Prompter`].

use crate::{Executor, ExecutorError, TextOutput};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Read, process::Stdio};

pub use asimov_patterns::PrompterOptions;
pub use asimov_prompt::{Prompt, PromptMessage, PromptRole};

/// The complete UTF-8 stdout of a successful [`Prompter`], or an execution error.
///
/// Whitespace and trailing newlines are preserved. Invalid UTF-8 is reported as
/// [`ExecutorError::UnexpectedOther`].
pub type PrompterResult = std::result::Result<String, ExecutorError>;

/// An external [prompter] that provides LLM inference for a [`Prompt`].
///
/// Each execution clones the stored prompt and writes its `Display`
/// representation to stdin in a separate Tokio task while the child's output
/// is collected. Stdout and stderr are always piped: the supplied [`TextOutput`]
/// is currently stored but does not affect output handling.
///
/// The response is buffered in full and decoded as UTF-8 without trimming or
/// further parsing. Format and model options are forwarded to the child; they
/// do not change how this wrapper serializes the prompt or decodes the response.
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
    /// `--model=<model>` arguments, followed by `options.other`. All three
    /// standard streams are piped, regardless of `output`.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: Prompt,
        output: TextOutput,
        options: PrompterOptions,
    ) -> Self {
        let mut executor = Executor::new(program);

        executor
            .command()
            .args(if let Some(ref input) = options.input {
                vec![format!("--input={}", input)]
            } else {
                vec![]
            })
            .args(if let Some(ref output) = options.output {
                vec![format!("--output={}", output)]
            } else {
                vec![]
            })
            .args(if let Some(ref model) = options.model {
                vec![format!("--model={}", model)]
            } else {
                vec![]
            })
            .args(&options.other)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
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
    /// Repeated calls send the same prompt again. Prompt writing happens in a
    /// spawned task whose handle is not awaited; a write failure panics in that
    /// task rather than being returned through this method's result.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning or waiting fails, if the child
    /// exits unsuccessfully, or if its stdout is not valid UTF-8.
    pub async fn execute(&mut self) -> PrompterResult {
        let mut process = self.executor.spawn().await?;

        let prompt = self.input.clone();
        let mut stdin = process.stdin.take().expect("should capture stdin");
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(prompt.to_string().as_bytes())
                .await
                .expect("should write to stdin");
        });

        let mut stdout = self.executor.wait(process).await?;
        let mut result = String::new();
        stdout.read_to_string(&mut result)?;

        Ok(result)
    }
}

impl asimov_patterns::Prompter<String, ExecutorError> for Prompter {}

#[async_trait]
impl asimov_patterns::Execute<String, ExecutorError> for Prompter {
    async fn execute(&mut self) -> PrompterResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            TextOutput::Ignored,
            PrompterOptions::default(),
        );
        let result = prompter.execute().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), String::from("user: Hello, world!\n"));
    }
}
