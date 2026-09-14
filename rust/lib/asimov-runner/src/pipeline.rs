// This is free and unencumbered software released into the public domain.

//! Placeholder for composition of multiple program executions.
//!
//! [`Pipeline`] currently has no stages, stream connections, or execution API.

/// A zero-sized placeholder for a future program pipeline.
///
/// This type can be constructed and cloned, but it does not yet store programs
/// or implement execution. Programs must currently be run individually.
#[derive(Clone, Debug)]
pub struct Pipeline;

impl Pipeline {}

#[cfg(test)]
mod tests {
    //use super::*;

    #[tokio::test]
    async fn test_construct() {
        // TODO
    }
}
