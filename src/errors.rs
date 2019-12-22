use futures::sync;
use std::collections::HashMap;
use tokio_threadpool::BlockingError;
use tungstenite::error::Error;

use crate::order_new;

type OrderReaderError = sync::mpsc::SendError<(
    order_new::Order,
    sync::oneshot::Sender<std::result::Result<std::vec::Vec<order_new::UnconditionalOrder>, AssetHandlingError>>,
)>;
#[derive(Debug)]
pub enum ServerError {
    OneshotCancelled(sync::oneshot::Canceled),
    PipelineError(OrderReaderError),
    WebSocketError(Error),
    DeserializationError(capnp::Error),
    AssetHandlingError(AssetHandlingError),
    ResponseError(sync::mpsc::SendError<tungstenite::protocol::Message>),
    AssetDoesNotExist
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
    Trades(sync::mpsc::SendError<HashMap<order_new::Order, Vec<order_new::UnconditionalOrder>>>),
    Orders(sync::mpsc::SendError<order_new::Order>),
}

#[derive(Debug)]
pub enum DatabaseError {
    ClientError(tokio_postgres::error::Error),
    ChannelError(()),
}
