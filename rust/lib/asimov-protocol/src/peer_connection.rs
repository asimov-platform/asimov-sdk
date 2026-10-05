// This is free and unencumbered software released into the public domain.

#![allow(dead_code)]

use crate::{Message, MessageRecv, MessageSend, PeerHello, PingError, RecvError, SendError};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use tokio::time::{Duration, Instant};

#[derive(Debug)]
pub struct PeerConnection {
    pub(crate) inner: Connection,
    pub(crate) send: SendStream,
    pub(crate) recv: RecvStream,
    pub(crate) hello: PeerHello,
}

impl PeerConnection {
    pub fn hello(&self) -> &PeerHello {
        return &self.hello;
    }

    /// Measure ping latency, rejecting any response other than a ping.
    pub async fn ping(&mut self) -> Result<Duration, PingError> {
        ping_transport(self).await
    }

    // pub fn close(self) -> Result<PeerConnection<Closed>, Infallible> {
    //     let Ready { inner, .. } = self.0;
    //     inner.close(0u32.into(), &[]);
    //     Ok(PeerConnection(Closed { inner }))
    // }
}

async fn ping_transport(
    connection: &mut (impl MessageSend + MessageRecv),
) -> Result<Duration, PingError> {
    let start = Instant::now();
    connection.send(Message::Ping).await?;
    let response = connection.recv().await?;
    if response != Message::Ping {
        return Err(PingError::UnexpectedResponse(response));
    }
    Ok(start.elapsed())
}

impl MessageSend for PeerConnection {
    async fn write_all(&mut self, buffer: &[u8]) -> Result<(), SendError> {
        Ok(self.send.write_all(buffer).await?)
    }
}

impl MessageRecv for PeerConnection {
    async fn read_exact(&mut self, buffer: &mut [u8]) -> Result<(), RecvError> {
        Ok(self.recv.read_exact(buffer).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_transport::MemoryTransport;
    use alloc::vec;

    #[tokio::test]
    async fn ping_rejects_unexpected_responses_without_panicking() {
        for response in [
            Message::Ping,
            Message::Bye,
            Message::Hello(PeerHello::default()),
            Message::List(vec![]),
            Message::Blob(asimov_kb::BlobId::from([0; 32])),
        ] {
            let valid = response == Message::Ping;
            let mut transport = MemoryTransport::default();
            transport.send(response).await.unwrap();
            transport.input = core::mem::take(&mut transport.output);
            let result = ping_transport(&mut transport).await;
            if valid {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(PingError::UnexpectedResponse(_))));
            }
            assert_eq!(transport.output, [0, 0, 0, 1, 0]);
        }
    }

    #[tokio::test]
    async fn ping_preserves_receive_failures() {
        let mut transport = MemoryTransport::default();
        assert!(matches!(
            ping_transport(&mut transport).await,
            Err(PingError::RecvPing(RecvError::Transport(_)))
        ));
    }
}
