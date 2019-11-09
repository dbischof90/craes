use std::collections::HashMap;
use std::env;
use std::io::Read;
use std::sync::{Arc, RwLock};

use futures::{prelude::*, sink::Sink, stream::Stream, sync};
use tracing::{debug, error, field, info, span, Level};
use tracing_fmt;
use tracing_futures;
use tracing_futures::Instrument;

use tokio::net::TcpListener;

use tokio_tungstenite::accept_async;
use tungstenite::error::Error;
use tungstenite::protocol::Message;

use capnp;
use capnp_futures;

pub mod ordermsg_capnp {
    include!(concat!(env!("OUT_DIR"), "/ordermsg_capnp.rs"));
}

mod order;
mod orderbook;

type OrderReaderError = sync::mpsc::SendError<(
    capnp::message::TypedReader<
        capnp_futures::serialize::OwnedSegments,
        ordermsg_capnp::order_msg::Owned,
    >,
    sync::oneshot::Sender<std::result::Result<(), AssetHandlingError>>,
)>;

#[derive(Debug)]
enum ServerError {
    OneshotCancelled(sync::oneshot::Canceled),
    PipelineError(OrderReaderError),
    WebSocketError(Error),
    DeserializationError(capnp::Error),
    AssetHandlingError(AssetHandlingError),
    ResponseError(sync::mpsc::SendError<tungstenite::protocol::Message>),
}

#[derive(Debug)]
enum AssetHandlingError {
    PhantomError(()),
    DeserializationError(capnp::Error),
}

fn main() {
    let server_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string())
        .parse()
        .unwrap();

    let socket = TcpListener::bind(&server_addr).unwrap();

    println!("Listening on: {}", server_addr);
    let mut threadpool = tokio::runtime::Runtime::new().unwrap();
    let mut addr_maps = HashMap::new();

    for i in 0..3 {
        let (numberchannel_tx, numberchannel_rx) = sync::mpsc::channel::<(
            capnp::message::TypedReader<
                capnp_futures::serialize::OwnedSegments,
                ordermsg_capnp::order_msg::Owned,
            >,
            sync::oneshot::Sender<std::result::Result<(), AssetHandlingError>>,
        )>(0);
        addr_maps.insert(i, numberchannel_tx);
        let mut ob = orderbook::Orderbook::new(i.to_string());

        let handle_asset = numberchannel_rx
            .map_err(AssetHandlingError::PhantomError)
            .and_then(|(typed_reader, os_tx)| {
                let order_reader = match typed_reader.get() {
                    Ok(r) => r,
                    Err(e) => {
                        os_tx
                            .send(Err(AssetHandlingError::DeserializationError(e.clone())))
                            .expect("Router was dropped!");
                        return Err(AssetHandlingError::DeserializationError(e));
                    }
                };

                let ordercondition = match order_reader.get_condition().which() {
                    Ok(ordermsg_capnp::order_msg::condition::Which::Unconditional(())) => {
                        order::OrderCondition::Unconditional
                    }
                    Ok(ordermsg_capnp::order_msg::condition::Which::Stoporder(val)) => {
                        order::OrderCondition::Stop {
                            stop: val.get_stop(),
                        }
                    }
                    _ => order::OrderCondition::Unconditional,
                };

                let limitprice = match order_reader.get_limitprice().which() {
                    Ok(ordermsg_capnp::order_msg::limitprice::Which::None(())) => None,
                    Ok(ordermsg_capnp::order_msg::limitprice::Which::Some(val)) => Some(val),
                    _ => None,
                };

                let order_to_process = order::Order::new(
                    0,
                    order_reader.get_buy(),
                    order_reader.get_volume(),
                    limitprice,
                    ordercondition,
                );

                Ok((order_to_process, os_tx))
            })
            .for_each(move |(j, os_tx)| {
                println!("Order: {:#?}", j);
                let trades = ob.resolve_order(j);
                println!("Trades: {:#?}", trades);
                os_tx.send(Ok(())).unwrap();
                Ok(())
            });

        threadpool.spawn(handle_asset.map_err(drop));
    }

    let connections = Arc::new(RwLock::new(addr_maps));

    let srv = socket
        .incoming()
        .map_err(|e| {
            //println!(msg = "Error accepting socket", error = field::display(&e));
            panic!("Error accepting socket");
        })
        .map(move |stream| {
            let addr = stream.peer_addr().unwrap();
            let conn_for_client = connections.clone();
            accept_async(stream)
                .map_err(|e| println!("Handshake failed: {:#?}", e))
                .and_then(move |ws_stream| {
                    println!("New connection from {}", addr);
                    let (response_to_client, order_stream) = ws_stream.split();
                    let (tx, rx) = sync::mpsc::unbounded();

                    let handle_msgs = order_stream
                        .map_err(|e| ServerError::WebSocketError(e))
                        .filter_map(|msg| match msg {
                            tungstenite::protocol::Message::Binary(load) => Some(load),
                            _ => None,
                        })
                        .and_then(move |load| {
                            capnp_futures::serialize::read_message(
                                std::io::Cursor::new(load),
                                capnp::message::ReaderOptions::new(),
                            )
                            .map_err(ServerError::DeserializationError)
                        })
                        .filter_map(|(_, root_cont_opt)| root_cont_opt)
                        .map(
                            capnp::message::TypedReader::<
                                capnp_futures::serialize::OwnedSegments,
                                ordermsg_capnp::order_msg::Owned,
                            >::from,
                        )
                        .and_then(move |msg| {
                            let (tx_os, rx_os) = futures::sync::oneshot::channel::<
                                std::result::Result<(), AssetHandlingError>,
                            >();
                            let tx_local = conn_for_client.read().unwrap().get(&1).unwrap().clone();
                            tx_local
                                .send((msg, tx_os))
                                .map_err(ServerError::PipelineError)
                                .and_then(move |_| rx_os.map_err(ServerError::OneshotCancelled))
                        })
                        .and_then(move |reply| {
                            println!("At the end: {:#?}", reply);
                            //let x = reply.unwrap().unwrap(); //Yeah, stuffy-stuff here.
                            match reply {
                                Ok(x) => tx
                                    .unbounded_send(tungstenite::Message::text("And back!"))
                                    .map_err(ServerError::ResponseError)
                                    .map(|x| Some(())),
                                Err(x) => Ok(None),
                            }
                        })
                        .for_each(|_| Ok(()));

                    handle_msgs
                        .or_else(|_| Ok(()))
                        .join(
                            response_to_client
                                .send_all(
                                    rx.map_err(|e| std::io::Error::from(std::io::ErrorKind::Other)),
                                )
                                .map(drop)
                                .map_err(drop),
                        )
                        .and_then(move |(_, _)| {
                            println!("Connection {} closed.", addr);
                            Ok(())
                        })
                })
        })
        .buffer_unordered(1000)
        .for_each(|_| Ok(()));

    let subscriber = tracing_fmt::FmtSubscriber::builder().finish();
    tracing::subscriber::with_default(subscriber, || {
        // Execute server.
        threadpool.spawn(srv);
        threadpool.shutdown_on_idle().wait().unwrap();
    });
}
