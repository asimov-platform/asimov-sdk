// This is free and unencumbered software released into the public domain.

use alloc::vec::Vec;
use serde::{Deserialize, Deserializer, Serialize};

type String = heapless::String<32>;

#[derive(Clone, Debug, Eq, Serialize)]
#[serde(untagged)]
pub enum NodeFeatureSet<'a> {
    Borrowed(&'a [&'a str]),
    Owned(Vec<String>),
}

impl PartialEq for NodeFeatureSet<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<'a> NodeFeatureSet<'a> {
    pub fn iter(&'a self) -> FeatureIter<'a> {
        match self {
            NodeFeatureSet::Borrowed(slice) => FeatureIter::Borrowed(slice.iter()),
            NodeFeatureSet::Owned(vec) => FeatureIter::Owned(vec.iter()),
        }
    }
}

impl<'de, 'a> Deserialize<'de> for NodeFeatureSet<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = Vec::<String>::deserialize(deserializer)?;
        Ok(NodeFeatureSet::Owned(vec))
    }
}

pub enum FeatureIter<'a> {
    Borrowed(core::slice::Iter<'a, &'a str>),
    Owned(core::slice::Iter<'a, String>),
}

impl<'a> Iterator for FeatureIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            FeatureIter::Borrowed(iter) => iter.next().copied(),
            FeatureIter::Owned(iter) => iter.next().map(|s| s.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, MessageRecv, MessageSend, PeerHello, test_transport::MemoryTransport};

    #[tokio::test]
    async fn hello_equality_survives_owned_deserialization() {
        for features in [&[][..], &["fetch", "list"][..]] {
            let borrowed = NodeFeatureSet::Borrowed(features);
            let owned = NodeFeatureSet::Owned(
                features
                    .iter()
                    .map(|feature| String::try_from(*feature).unwrap())
                    .collect(),
            );
            assert_eq!(borrowed, owned);
            assert_eq!(owned, borrowed);
            assert_ne!(owned, NodeFeatureSet::Borrowed(&["other"]));

            let hello = Message::Hello(PeerHello {
                required_features: borrowed.clone(),
                supported_features: borrowed,
                ..Default::default()
            });
            let mut transport = MemoryTransport::default();
            transport.send(hello.clone()).await.unwrap();
            transport.input = core::mem::take(&mut transport.output);
            assert_eq!(transport.recv().await.unwrap(), hello);
        }
    }
}
