// This is free and unencumbered software released into the public domain.

use crate::Message;
use alloc::boxed::Box;
use core::error::Error;
use heapless::CapacityError;
use known_errors::sysexits::SysexitsError;
use thiserror::Error;

pub type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AcceptError {
    #[error(transparent)]
    Transport(#[from] iroh::protocol::AcceptError),

    #[error(transparent)]
    Connection(#[from] iroh::endpoint::ConnectionError),

    #[error(transparent)]
    RecvHello(#[from] RecvError),

    #[error(transparent)]
    SendHello(#[from] SendError),

    #[error("invalid message from peer")]
    InvalidMessage(Message),
}

impl From<AcceptError> for SysexitsError {
    fn from(_: AcceptError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BindError {
    #[error(transparent)]
    Transport(#[from] iroh::endpoint::BindError),
}

impl From<BindError> for SysexitsError {
    fn from(_: BindError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConnectError {
    #[error(transparent)]
    Transport(#[from] iroh::endpoint::ConnectError),

    #[error(transparent)]
    Connection(#[from] iroh::endpoint::ConnectionError),

    #[error(transparent)]
    SendHello(#[from] SendError),

    #[error(transparent)]
    RecvHello(#[from] RecvError),

    #[error("invalid message from peer")]
    InvalidMessage(Message),
}

impl From<ConnectError> for SysexitsError {
    fn from(_: ConnectError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PingError {
    #[error(transparent)]
    ConnectPeer(#[from] ConnectError),

    #[error(transparent)]
    SendPing(#[from] SendError),

    #[error(transparent)]
    RecvPing(#[from] RecvError),

    #[error(transparent)]
    Other(#[from] BoxError),
}

impl From<PingError> for SysexitsError {
    fn from(_: PingError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PublishError {
    #[error(transparent)]
    Gossip(#[from] iroh_gossip::api::ApiError),

    #[error(transparent)]
    Other(#[from] BoxError),
}

impl From<PublishError> for SysexitsError {
    fn from(_: PublishError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StartError {
    #[error(transparent)]
    Other(#[from] BoxError),
}

impl From<StartError> for SysexitsError {
    fn from(_: StartError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SubscribeError {
    #[error(transparent)]
    Gossip(#[from] iroh_gossip::api::ApiError),

    #[error(transparent)]
    Other(#[from] BoxError),
}

impl From<SubscribeError> for SysexitsError {
    fn from(_: SubscribeError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TerminateError {
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}

impl From<TerminateError> for SysexitsError {
    fn from(_: TerminateError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SendError {
    #[error(transparent)]
    Serialize(#[from] postcard::Error),

    #[error(transparent)]
    Transport(#[from] iroh::endpoint::WriteError),
}

impl From<SendError> for SysexitsError {
    fn from(_: SendError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RecvError {
    #[error(transparent)]
    Transport(#[from] iroh::endpoint::ReadExactError),

    #[error(transparent)]
    Capacity(#[from] CapacityError),

    #[error(transparent)]
    Deserialize(#[from] postcard::Error),
}

impl From<RecvError> for SysexitsError {
    fn from(_: RecvError) -> Self {
        SysexitsError::EX_SOFTWARE // TODO
    }
}
