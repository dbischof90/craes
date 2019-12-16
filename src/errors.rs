use futures::sync;
use std::collections::HashMap;
use tokio_threadpool::BlockingError;
use tungstenite::error::Error;

use crate::order;

type OrderReaderError = sync::mpsc::SendError<(
    order::Order,
    sync::oneshot::Sender<std::result::Result<std::vec::Vec<order::Order>, AssetHandlingError>>,
)>;
#[derive(Debug)]
pub enum ServerError {
    OneshotCancelled(sync::oneshot::Canceled),
    PipelineError(OrderReaderError),
    WebSocketError(Error),
    DeserializationError(capnp::Error),
    AssetHandlingError(AssetHandlingError),
    ResponseError(sync::mpsc::SendError<tungstenite::protocol::Message>),
}

#[derive(Debug)]
pub enum AssetHandlingError {
    PhantomError(()),
    RouterHasShutDown,
    OrderbookInsertError(BlockingError),
    SendingToDatabase(DatabaseChannelError),
}

#[derive(Debug)]
pub enum DatabaseChannelError {
    Trades(sync::mpsc::SendError<HashMap<order::Order, Vec<order::Order>>>),
    Orders(sync::mpsc::SendError<order::Order>),
}

#[derive(Debug)]
pub enum DatabaseError {
    ClientError(tokio_postgres::error::Error),
    ChannelError(()),
}
