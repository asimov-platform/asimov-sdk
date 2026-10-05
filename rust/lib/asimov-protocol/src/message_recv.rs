// This is free and unencumbered software released into the public domain.

use crate::{MAX_MESSAGE_BODY_LEN, MESSAGE_HEADER_LEN, Message, MessageLen, RecvError};

pub trait MessageRecv {
    fn recv(&mut self) -> impl Future<Output = Result<Message, RecvError>> {
        async {
            let mut head_buffer = [0u8; MESSAGE_HEADER_LEN];
            self.read_exact(&mut head_buffer).await?;

            let body_len = MessageLen::from_be_bytes(head_buffer);
            if body_len > MAX_MESSAGE_BODY_LEN as MessageLen {
                return Err(RecvError::MessageTooLarge {
                    length: body_len,
                    limit: MAX_MESSAGE_BODY_LEN,
                });
            }

            let mut buffer = [0u8; MAX_MESSAGE_BODY_LEN];
            let body = &mut buffer[..body_len as usize];
            self.read_exact(body).await?;

            let response: Message = postcard::from_bytes(body)?;

            Ok(response)
        }
    }

    fn read_exact(&mut self, buffer: &mut [u8]) -> impl Future<Output = Result<(), RecvError>>;
}
