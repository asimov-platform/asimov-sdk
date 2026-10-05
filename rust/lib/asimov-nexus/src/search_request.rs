// This is free and unencumbered software released into the public domain.

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

/// A search request with an `N`-dimensional query vector.
///
/// With the `validator` feature, `validator::Validate::validate` checks the
/// vector dimension, finite components, and optional score/count bounds.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct SearchRequest<const N: usize = 128> {
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Exactly `N` finite components.
    pub vector: Vec<f32>,

    /// A finite minimum score in the inclusive range `0.0..=1.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_score: Option<f32>,

    /// A maximum result count in the inclusive range `1..=10`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_count: Option<u8>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_credits: Option<usize>,
}

#[cfg(feature = "validator")]
impl<const N: usize> validator::Validate for SearchRequest<N> {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        use validator::{ValidationError, ValidationErrors};

        let mut errors = ValidationErrors::new();
        if self.vector.len() != N {
            let mut error = ValidationError::new("length");
            error.add_param("equal".into(), &N);
            error.add_param("value".into(), &self.vector.len());
            errors.add("vector", error);
        }
        if self.vector.iter().any(|value| !value.is_finite()) {
            errors.add("vector", ValidationError::new("finite"));
        }
        if let Some(score) = self.min_score {
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                let mut error = ValidationError::new("range");
                error.add_param("min".into(), &0.0_f32);
                error.add_param("max".into(), &1.0_f32);
                error.add_param("value".into(), &score);
                errors.add("min_score", error);
            }
        }
        if let Some(count) = self.max_count {
            if !(1..=10).contains(&count) {
                let mut error = ValidationError::new("range");
                error.add_param("min".into(), &1_u8);
                error.add_param("max".into(), &10_u8);
                error.add_param("value".into(), &count);
                errors.add("max_count", error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(all(test, feature = "validator"))]
mod tests {
    use super::*;
    use alloc::vec;
    use validator::Validate;

    #[test]
    fn validation_uses_the_declared_dimension() {
        let default_dimension: SearchRequest = SearchRequest {
            vector: vec![0.0; 128],
            ..Default::default()
        };
        assert!(default_dimension.validate().is_ok());

        let mut request = SearchRequest::<3> {
            vector: vec![0.0; 3],
            ..Default::default()
        };
        assert!(request.validate().is_ok());
        for length in [0, 2, 4, 128] {
            request.vector = vec![0.0; length];
            let errors = request.validate().unwrap_err();
            assert_eq!(errors.field_errors()["vector"][0].code, "length");
        }
    }

    #[test]
    fn validation_rejects_non_finite_components() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let request = SearchRequest::<3> {
                vector: vec![0.0, value, 1.0],
                ..Default::default()
            };
            let errors = request.validate().unwrap_err();
            assert_eq!(errors.field_errors()["vector"][0].code, "finite");
        }
    }

    #[test]
    fn validation_checks_optional_bounds_and_collects_errors() {
        let mut request = SearchRequest::<1> {
            vector: vec![0.0],
            ..Default::default()
        };
        assert!(request.validate().is_ok());
        for score in [0.0, 0.5, 1.0] {
            request.min_score = Some(score);
            for count in [1, 5, 10] {
                request.max_count = Some(count);
                assert!(request.validate().is_ok());
            }
        }
        for score in [-0.1, 1.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            request.min_score = Some(score);
            let errors = request.validate().unwrap_err();
            assert_eq!(errors.field_errors()["min_score"][0].code, "range");
        }
        request.vector.clear();
        request.min_score = Some(-0.1);
        for count in [0, 11, u8::MAX] {
            request.max_count = Some(count);
            let errors = request.validate().unwrap_err();
            let fields = errors.field_errors();
            assert_eq!(fields.len(), 3);
            assert_eq!(fields["vector"][0].code, "length");
            assert_eq!(fields["min_score"][0].code, "range");
            assert_eq!(fields["max_count"][0].code, "range");
        }
    }
}
