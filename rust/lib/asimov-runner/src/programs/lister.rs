// This is free and unencumbered software released into the public domain.

use crate::{Executor, ExecutorError, GraphOutput};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::ListerOptions;

/// See: https://asimov-specs.github.io/program-patterns/#lister
pub type ListerResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// See: https://asimov-specs.github.io/program-patterns/#lister
#[allow(unused)]
#[derive(Debug)]
pub struct Lister {
    executor: Executor,
    options: ListerOptions,
    input: String,
    output: GraphOutput,
}

impl Lister {
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: ListerOptions,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(ref sort) = options.sort {
                vec![format!("--sort={}", sort.to_string())]
            } else {
                vec![]
            })
            .args(if let Some(offset) = options.offset {
                vec![format!("--offset={}", offset)]
            } else {
                vec![]
            })
            .args(if let Some(limit) = options.limit {
                vec![format!("--limit={}", limit)]
            } else {
                vec![]
            })
            .args(if let Some(ref output) = options.output {
                vec![format!("--output={}", output)]
            } else {
                vec![]
            })
            .args(&options.other)
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    pub async fn execute(&mut self) -> ListerResult {
        let stdout = self.executor.execute().await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Lister<Cursor<Vec<u8>>, ExecutorError> for Lister {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Lister {
    async fn execute(&mut self) -> ListerResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    //use super::*;
    //use asimov_patterns::Execute;

    #[tokio::test]
    async fn test_execute() {
        // TODO
    }
}
