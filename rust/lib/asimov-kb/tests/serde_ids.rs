// This is free and unencumbered software released into the public domain.

#![cfg(feature = "serde")]

use asimov_kb::{BlobId, EventId, Id, IdClass, OrganizationId, PersonId};

#[test]
fn typed_ids_reject_wrong_classes_and_lengths() {
    for class in [
        IdClass::Blob,
        IdClass::Event,
        IdClass::Organization,
        IdClass::Person,
    ] {
        for len in [16, 32] {
            let json = if len == 16 {
                serde_json::to_string(&Id::<16>::zero(class)).unwrap()
            } else {
                serde_json::to_string(&Id::<32>::zero(class)).unwrap()
            };
            assert_eq!(
                serde_json::from_str::<BlobId>(&json).is_ok(),
                class == IdClass::Blob && len == 32
            );
            assert_eq!(
                serde_json::from_str::<EventId>(&json).is_ok(),
                class == IdClass::Event && len == 16
            );
            assert_eq!(
                serde_json::from_str::<OrganizationId>(&json).is_ok(),
                class == IdClass::Organization && len == 16
            );
            assert_eq!(
                serde_json::from_str::<PersonId>(&json).is_ok(),
                class == IdClass::Person && len == 16
            );
        }
    }
}

fn round_trip<T>(id: T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + core::fmt::Debug + PartialEq,
{
    let json = serde_json::to_string(&id).unwrap();
    assert!(json.starts_with('"'));
    assert_eq!(serde_json::from_str::<T>(&json).unwrap(), id);
}

#[test]
fn typed_ids_preserve_their_string_wire_format() {
    for byte in [0, 1, 127, 255] {
        round_trip(BlobId::from([byte; 32]));
        round_trip(EventId::from([byte; 16]));
        round_trip(OrganizationId::from([byte; 16]));
        round_trip(PersonId::from([byte; 16]));
    }
}

#[test]
fn typed_ids_reject_malformed_input() {
    for json in [r#""""#, r#""P0""#, r#""🦀""#, "null", "42", "[]", "{}"] {
        assert!(serde_json::from_str::<BlobId>(json).is_err());
        assert!(serde_json::from_str::<EventId>(json).is_err());
        assert!(serde_json::from_str::<OrganizationId>(json).is_err());
        assert!(serde_json::from_str::<PersonId>(json).is_err());
    }
}
