// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]
#![allow(unused)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(test)]
mod test_transport;

#[cfg(feature = "std")]
extern crate std;

pub use futures_lite::{Stream, StreamExt};

#[doc(hidden)]
pub use iroh;

pub use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey,
    endpoint::presets::Preset,
    protocol::{Router, RouterBuilder},
};
pub use iroh_gossip::{
    TopicId,
    api::{GossipReceiver, GossipSender, GossipTopic},
};
pub use iroh_tickets::{ParseError as TicketParsingError, Ticket, endpoint::EndpointTicket};

pub use asimov_id::{Handle, HandleError, Id, IdError, KeyError, PublicKey, PublicKeyEncoding};

mod errors;
pub use errors::*;

mod gossip_protocol;
pub use gossip_protocol::*;

mod handle_resolvers;
pub use handle_resolvers::*;

mod message;
pub use message::*;

mod message_header;
pub use message_header::*;

mod message_recv;
pub use message_recv::*;

mod message_send;
pub use message_send::*;

mod node;
pub use node::*;

mod node_metrics;
pub use node_metrics::*;

pub mod node_state;

mod peer_accept;
pub use peer_accept::*;

pub mod peer_accept_state;

mod peer_connect;
pub use peer_connect::*;

pub mod peer_connect_state;

mod peer_connection;
pub use peer_connection::*;

pub mod peer_connection_state;

mod peer_feature_set;
pub use peer_feature_set::*;

mod peer_hello;
pub use peer_hello::*;

mod peer_id;
pub use peer_id::*;

mod peer_protocol;
pub use peer_protocol::*;

mod presets;
pub use presets::*;

mod resolve_handle;
pub use resolve_handle::*;

mod topic;
pub use topic::*;

mod topic_subscription;
pub use topic_subscription::*;
