// This is free and unencumbered software released into the public domain.

use crate::{MessageRecv, MessageSend, RecvError, SendError};
use alloc::vec::Vec;

#[derive(Default)]
pub(crate) struct MemoryTransport {
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    pub position: usize,
}

impl MessageRecv for MemoryTransport {
    async fn read_exact(&mut self, buffer: &mut [u8]) -> Result<(), RecvError> {
        let remaining = &self.input[self.position..];
        if remaining.len() < buffer.len() {
            return Err(iroh::endpoint::ReadExactError::FinishedEarly(remaining.len()).into());
        }
        buffer.copy_from_slice(&remaining[..buffer.len()]);
        self.position += buffer.len();
        Ok(())
    }
}

impl MessageSend for MemoryTransport {
    async fn write_all(&mut self, buffer: &[u8]) -> Result<(), SendError> {
        self.output.extend_from_slice(buffer);
        Ok(())
    }
}

#[tokio::test]
async fn framing_round_trips_every_message_and_the_size_limit() {
    use crate::{MAX_MESSAGE_BODY_LEN, MESSAGE_HEADER_LEN, Message, PeerHello};
    use alloc::vec;

    for message in [
        Message::Ping,
        Message::Bye,
        Message::Hello(PeerHello {
            required_features: crate::NodeFeatureSet::Owned(vec![]),
            supported_features: crate::NodeFeatureSet::Owned(vec![]),
            ..Default::default()
        }),
        Message::Blob(asimov_kb::BlobId::from([42; 32])),
        Message::List(vec!["x".repeat(MAX_MESSAGE_BODY_LEN - 4)]),
    ] {
        let mut transport = MemoryTransport::default();
        let written = transport.send(message.clone()).await.unwrap();
        if matches!(message, Message::List(_)) {
            assert_eq!(written, MESSAGE_HEADER_LEN + MAX_MESSAGE_BODY_LEN);
        }
        transport.input = core::mem::take(&mut transport.output);
        assert_eq!(transport.recv().await.unwrap(), message);
        assert_eq!(transport.position, written);
    }
}

#[tokio::test]
async fn oversized_messages_fail_before_body_io() {
    use crate::{MAX_MESSAGE_BODY_LEN, MESSAGE_HEADER_LEN, Message, MessageLen};
    use alloc::vec;

    for length in [MAX_MESSAGE_BODY_LEN as MessageLen + 1, MessageLen::MAX] {
        let mut transport = MemoryTransport {
            input: length.to_be_bytes().into(),
            ..Default::default()
        };
        assert!(matches!(transport.recv().await,
            Err(RecvError::MessageTooLarge { length: actual, .. }) if actual == length));
        assert_eq!(transport.position, MESSAGE_HEADER_LEN);
    }

    let mut transport = MemoryTransport::default();
    let message = Message::List(vec!["x".repeat(MAX_MESSAGE_BODY_LEN - 3)]);
    assert!(matches!(
        transport.send(message).await,
        Err(SendError::MessageTooLarge { .. })
    ));
    assert!(transport.output.is_empty());
}

#[tokio::test]
async fn truncated_and_malformed_frames_return_errors() {
    use alloc::vec;

    for input in [vec![0, 0], vec![0, 0, 0, 10, 0]] {
        let mut transport = MemoryTransport {
            input,
            ..Default::default()
        };
        assert!(matches!(
            transport.recv().await,
            Err(RecvError::Transport(_))
        ));
    }
    for input in [vec![0, 0, 0, 0], vec![0, 0, 0, 1, 255]] {
        let mut transport = MemoryTransport {
            input,
            ..Default::default()
        };
        assert!(matches!(
            transport.recv().await,
            Err(RecvError::Deserialize(_))
        ));
    }
}
