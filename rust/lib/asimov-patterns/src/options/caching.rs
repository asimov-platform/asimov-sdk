// This is free and unencumbered software released into the public domain.

use bon::Builder;

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct CachingOptions {}
