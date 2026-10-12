// This is free and unencumbered software released into the public domain.

extern crate alloc;

use alloc::{string::ToString, vec, vec::Vec};
use asimov_kb::{BlobId, EventId, Id, IdClass, IdError, OrganizationId, PersonId};

fn check_generic<const N: usize>() {
    for class in [
        IdClass::Blob,
        IdClass::Event,
        IdClass::Organization,
        IdClass::Person,
    ] {
        for length in [0, N.saturating_sub(1), N, N + 1, N + 32] {
            let bytes: Vec<u8> = (0..length).map(|i| i as u8).collect();
            let from_vec = Id::<N>::try_from((class, &bytes));
            let from_slice = Id::<N>::try_from((class, bytes.as_slice()));
            assert_eq!(from_vec, from_slice);
            if length == N {
                let id = from_vec.unwrap();
                assert_eq!(id.class(), class);
                assert_eq!(id.as_bytes(), bytes);
                assert_eq!(id.to_string().parse::<Id<N>>(), Ok(id));
            } else {
                assert_eq!(from_vec, Err(IdError::InvalidLength));
            }
        }
    }
}

#[test]
fn generic_ids_require_exact_length() {
    check_generic::<0>();
    check_generic::<1>();
    check_generic::<16>();
    check_generic::<32>();
    check_generic::<64>();
}

macro_rules! check_typed {
    ($test:ident, $ty:ident, $class:ident, $length:literal) => {
        #[test]
        fn $test() {
            for length in [0, 1, $length - 1, $length + 1, $length * 2] {
                let bytes = vec![42; length];
                assert_eq!($ty::try_from(&bytes), Err(IdError::InvalidLength));
                assert_eq!($ty::try_from(bytes.as_slice()), Err(IdError::InvalidLength));
            }

            for bytes in [
                [0; $length],
                [255; $length],
                core::array::from_fn(|i| i as u8),
            ] {
                let expected = $ty::from(bytes);
                assert_eq!(expected.as_id().class(), IdClass::$class);
                assert_eq!(expected.as_id().as_bytes(), bytes);
                assert_eq!($ty::try_from(&bytes.to_vec()), Ok(expected.clone()));
                assert_eq!($ty::try_from(bytes.as_slice()), Ok(expected.clone()));
                assert_eq!(expected.to_string().parse::<$ty>(), Ok(expected));
            }
        }
    };
}

check_typed!(blob_ids_require_exact_length, BlobId, Blob, 32);
check_typed!(event_ids_require_exact_length, EventId, Event, 16);
check_typed!(person_ids_require_exact_length, PersonId, Person, 16);
check_typed!(
    organization_ids_require_exact_length,
    OrganizationId,
    Organization,
    16
);

#[cfg(feature = "uuid")]
#[test]
fn uuid_conversions_preserve_identifier_bytes() {
    for bytes in [[0; 16], [255; 16], core::array::from_fn(|i| i as u8)] {
        let id = Id::from((IdClass::Person, bytes));
        let uuid: dogma_uuid::Uuid = id.as_uuid();
        assert_eq!(uuid.as_bytes(), &bytes);
        assert_eq!(id.into_uuid(), uuid);
    }
}

#[cfg(all(feature = "uuid", feature = "std"))]
#[test]
fn generated_ids_preserve_class_and_base58_encoding() {
    for (id, class) in [
        (EventId::new().into_id(), IdClass::Event),
        (OrganizationId::new().into_id(), IdClass::Organization),
        (PersonId::new().into_id(), IdClass::Person),
    ] {
        assert_eq!(id.class(), class);
        assert_eq!(id.as_bytes()[6] >> 4, 7);
        assert_eq!(id.as_bytes()[8] >> 6, 0b10);
        let encoded = id.to_string();
        let decoded = bs58::decode(&encoded[1..]).into_vec().unwrap();
        assert_eq!(decoded, id.as_uuid().as_bytes());
        assert_eq!(encoded.parse::<Id>().unwrap(), id);
    }
}
