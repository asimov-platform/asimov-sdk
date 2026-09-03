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

pub use asimov_patterns::CatalogerOptions;

/// See: https://asimov-specs.github.io/program-patterns/#cataloger
pub type CatalogerResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// See: https://asimov-specs.github.io/program-patterns/#cataloger
#[allow(unused)]
#[derive(Debug)]
pub struct Cataloger {
    executor: Executor,
    options: CatalogerOptions,
    input: String,
    output: GraphOutput,
}

impl Cataloger {
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: CatalogerOptions,
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

    pub async fn execute(&mut self) -> CatalogerResult {
        let stdout = self.executor.execute().await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Cataloger<Cursor<Vec<u8>>, ExecutorError> for Cataloger {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Cataloger {
    async fn execute(&mut self) -> CatalogerResult {
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
