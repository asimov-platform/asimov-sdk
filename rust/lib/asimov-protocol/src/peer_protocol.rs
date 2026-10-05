// This is free and unencumbered software released into the public domain.

//! The peer-to-peer protocol.

use crate::{Message, MessageRecv, MessageSend, NodeMetrics, PeerAccept, ProtocolMessageError};
use alloc::sync::Arc;
use asimov_id::PublicKey;
use iroh::{
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};

/// The ALPN string for the node protocol.
pub const NODE_ALPN: &[u8] = b"asimov/node";

/// The node protocol for use with `Router`.
#[derive(Debug, Clone)]
pub struct NodeProtocol {
    /// Shared state for use across incoming connections.
    metrics: Arc<NodeMetrics>,
}

impl Default for NodeProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeProtocol {
    /// Creates a new node protocol state.
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(NodeMetrics::default()),
        }
    }

    /// Returns a handle to the node metrics.
    pub fn metrics(&self) -> &Arc<NodeMetrics> {
        &self.metrics
    }

    fn response(&self, request: Message) -> Result<Message, ProtocolMessageError> {
        match request {
            Message::Ping => {
                self.metrics.pings_recv.inc();
                Ok(Message::Ping)
            },
            Message::Bye => Ok(Message::Bye),
            Message::Hello(_) => Err(ProtocolMessageError::Unexpected(request)),
            _ => Err(ProtocolMessageError::Unsupported(request)),
        }
    }
}

impl ProtocolHandler for NodeProtocol {
    /// Each incoming connection for our ALPN results in a call to `accept`.
    ///
    /// The returned future runs on a newly spawned Tokio task, so it can run
    /// indefinitely as long as the connection remains open.
    async fn accept(&self, connection: Connection) -> n0_error::Result<(), AcceptError> {
        let node_id: PublicKey = connection.remote_id().into();

        #[cfg(feature = "std")]
        std::eprintln!("Accepted a connection from node {node_id}"); // DEBUG

        // Expect the connecting peer to open a bidirectional QUIC stream:
        let state: PeerAccept = connection.into();
        let state = state.recv_hello().await.map_err(AcceptError::from_err)?;
        let state = state.send_hello().await.map_err(AcceptError::from_err)?;

        let mut connection = state.into_connection();

        loop {
            let request = connection.recv().await.map_err(AcceptError::from_err)?;
            let response = self.response(request).map_err(AcceptError::from_err)?;
            let is_bye = response == Message::Bye;
            connection
                .send(response)
                .await
                .map_err(AcceptError::from_err)?;
            if is_bye {
                break;
            }
        }

        // Send the response and finish the send stream:
        connection.send.finish()?;

        // Wait for the remote end to explicitly and gracefully close the
        // connection after receiving our response:
        connection.inner.closed().await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PeerHello, test_transport::MemoryTransport};
    use alloc::vec;

    #[tokio::test]
    async fn established_protocol_rejects_unsupported_and_out_of_order_messages() {
        let protocol = NodeProtocol::new();
        for request in [
            Message::Ping,
            Message::Bye,
            Message::Hello(PeerHello::default()),
            Message::List(vec!["example".into()]),
            Message::Blob(asimov_kb::BlobId::from([42; 32])),
        ] {
            let mut transport = MemoryTransport::default();
            transport.send(request.clone()).await.unwrap();
            transport.input = core::mem::take(&mut transport.output);
            let result = protocol.response(transport.recv().await.unwrap());
            match request {
                Message::Ping | Message::Bye => assert_eq!(result.unwrap(), request),
                Message::Hello(_) => {
                    assert!(matches!(result, Err(ProtocolMessageError::Unexpected(_))))
                },
                _ => assert!(matches!(result, Err(ProtocolMessageError::Unsupported(_)))),
            }
        }
    }
}
