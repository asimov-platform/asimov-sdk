// This is free and unencumbered software released into the public domain.

//! JSON Utilities
//!
//! See: <https://github.com/serde-rs/json/issues/513>
//! ```rust
//! use asimov_module::json::SkipNulls;
//!
//! let text = serde_json::to_string(&SkipNulls(serde_json::json!({
//!     "a": 1,
//!     "b": null,
//!     "c": 3
//! })));
//!
//! assert!(text.is_ok());
//! assert_eq!(text.unwrap(), r#"{"a":1,"c":3}"#);
//! ```

use serde::{
    Deserialize,
    ser::{Serialize, SerializeMap, SerializeSeq, Serializer},
};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct SkipNulls(pub Value);

impl Serialize for SkipNulls {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            Value::Null => serializer.serialize_unit(),
            Value::Array(input) => {
                let mut output = serializer.serialize_seq(Some(input.len()))?;
                for element in input {
                    if let Value::Null = element {
                        continue;
                    } else {
                        output.serialize_element(&SkipNulls(element.clone()))?;
                    }
                }
                output.end()
            },
            Value::Object(input) => {
                let mut output = serializer.serialize_map(Some(input.len()))?;
                for (key, value) in input {
                    if let Value::Null = value {
                        continue;
                    } else {
                        output.serialize_entry(&key, &SkipNulls(value.clone()))?;
                    }
                }
                output.end()
            },
            value => value.serialize(serializer),
        }
    }
}
