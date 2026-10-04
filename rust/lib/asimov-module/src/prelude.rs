// This is free and unencumbered software released into the public domain.

//! Legacy prelude retained for compatibility after Dogma 0.3 removed it.

pub use core::{
    hash::{self, Hash, Hasher},
    result::{self, Result},
};

pub use alloc::{
    borrow::{self, Borrow, BorrowMut, Cow, ToOwned},
    boxed::{self, Box},
    collections::{self, BTreeMap, BTreeSet, BinaryHeap, LinkedList, VecDeque},
    fmt, format,
    str::{self, FromStr},
    string::{self, String, ToString},
    vec::{self, IntoIter, Vec},
};

#[cfg(feature = "std")]
pub use std::collections::{HashMap, HashSet};
