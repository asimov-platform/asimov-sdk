// This is free and unencumbered software released into the public domain.

pub type MessageLen = u32;

pub const MESSAGE_HEADER_LEN: usize = size_of::<MessageLen>();

/// Maximum serialized message body size, excluding the length header.
pub const MAX_MESSAGE_BODY_LEN: usize = 1024;
