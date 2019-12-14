use std::collections::HashMap;
use std::env;
use std::sync::{atomic, Arc, Mutex, RwLock};

use futures::{
    future,
    prelude::*,
    sink::Sink,
    stream::{iter_ok, Stream},
    sync,
};
use tokio::net::TcpListener;
use tokio_threadpool::{blocking, BlockingError};

use tokio_tungstenite::accept_async;
use tungstenite::error::Error;

use tokio_postgres;

use capnp;
use capnp_futures;

mod order;
mod orderbook;
mod ordermsg_capnp {
    include!(concat!(env!("OUT_DIR"), "/ordermsg_capnp.rs"));
}

type OrderReaderError = sync::mpsc::SendError<(
    order::Order,
    sync::oneshot::Sender<std::result::Result<std::vec::Vec<order::Order>, AssetHandlingError>>,
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
    RouterHasShutDown,
    OrderbookInsertError(BlockingError),
    SendingToDatabase(DatabaseChannelError),
}

#[derive(Debug)]
enum DatabaseChannelError {
    Trades(sync::mpsc::SendError<HashMap<order::Order, Vec<order::Order>>>),
    Orders(sync::mpsc::SendError<order::Order>),
}

#[derive(Debug)]
enum DatabaseError {
    ClientError(tokio_postgres::error::Error),
    ChannelError(()),
}

fn copy_into_database<S>(
    db_name: String,
    lines_stream: S,
    threadpool: &mut tokio::runtime::Runtime,
) where
    S: Stream<Item = std::vec::Vec<u8>, Error = DatabaseError> + Send + 'static,
{
    let executor_handle = threadpool.executor();
    let fut = tokio_postgres::connect(
        "host=router user=postgres dbname=craes",
        tokio_postgres::NoTls,
    )
    .map(move |(client, connection)| {
        let connection = connection.map_err(|e| eprintln!("connection error: {}", e));
        executor_handle.spawn(connection);
        client
    })
    .and_then(move |mut client| {
        client
            .prepare(&["COPY ", &db_name, " FROM STDIN"].concat())
            .map(|statement| (client, statement))
    })
    .map_err(DatabaseError::ClientError)
    .and_then(|(mut client, statement)| {
        lines_stream
            .chunks(3)
            .and_then(move |prepared_rows| {
                let stream_rows = iter_ok::<_, std::io::Error>(prepared_rows);
                client
                    .copy_in(&statement, &[], stream_rows)
                    .map_err(DatabaseError::ClientError)
            })
            .for_each(|_| Ok(()))
    });

    threadpool.spawn(fut.map_err(drop));
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
    let order_id = Arc::new(atomic::AtomicI32::new(0));

    // Database setup
    let (dbchannel_tx, dbchannel_rx) = sync::mpsc::unbounded();
    let (order_db_tx, order_db_rx) = sync::mpsc::unbounded();

    let order_processing = order_db_rx
        .map(|received_order: order::Order| {
            let mut buf = [
                received_order.id.to_string(),
                received_order.buy.to_string(),
                received_order.volume.to_string(),
                received_order.limit_price.unwrap_or(0).to_string(), //fix...
                received_order.created_at.to_rfc3339(),
            ]
            .join("\t");
            buf.push('\n');
            buf.into_bytes()
        })
        .map_err(DatabaseError::ChannelError);

    let trade_processing = dbchannel_rx
        .map(
            |executed_trades: std::collections::HashMap<
                order::Order,
                std::vec::Vec<order::Order>,
            >| {
                let rows_iter =
                    executed_trades
                        .into_iter()
                        .flat_map(|(executed_order, matched_orders)| {
                            matched_orders.into_iter().map(move |next_matched_order| {
                                let mut buf = [
                                    executed_order.id.to_string(),
                                    next_matched_order.id.to_string(),
                                    next_matched_order.volume.to_string(),
                                    next_matched_order.limit_price.unwrap().to_string(),
                                    next_matched_order.created_at.to_string(),
                                    next_matched_order.filled_at.unwrap().to_rfc3339(),
                                ]
                                .join("\t");
                                buf.push('\n');
                                buf.into_bytes()
                            })
                        });
                iter_ok(rows_iter)
            },
        )
        .flatten()
        .map_err(DatabaseError::ChannelError);

    copy_into_database("trades".to_string(), trade_processing, &mut threadpool);
    copy_into_database("orders".to_string(), order_processing, &mut threadpool);

    //TODO: Set up orderbooks from database.
    for i in 0..3 {
        let dbchannel_tx_local = dbchannel_tx.clone();
        let order_db_tx_local = order_db_tx.clone();
        let (assetchannel_tx, assetchannel_rx) = sync::mpsc::channel::<(
            order::Order,
            sync::oneshot::Sender<
                std::result::Result<std::vec::Vec<order::Order>, AssetHandlingError>,
            >,
        )>(0);
        addr_maps.insert(i, assetchannel_tx);
        let ob = Arc::new(Mutex::new(orderbook::Orderbook::new(i.to_string())));

        let handle_asset = assetchannel_rx
            .map_err(AssetHandlingError::PhantomError)
            .and_then(move |(order_to_process, os_tx)| {
                let local_orderbook = ob.clone();
                println!("Orderbook: {:#?}", ob);
                future::poll_fn(move || {
                    blocking(|| {
                        //TODO: Can this or should this be able to fail controlled?
                        local_orderbook
                            .lock()
                            .unwrap()
                            .resolve_order(order_to_process)
                    })
                })
                .map_err(AssetHandlingError::OrderbookInsertError)
                .map(move |trades| (order_to_process, trades, os_tx))
            })
            .for_each(move |(processed_order, trades, os_tx)| {
                println!("Trades: {:#?}", trades);
                let own_executed_trades = match trades.get(&processed_order) {
                    Some(t) => t.clone(),
                    None => std::vec::Vec::new()
                };

                let sent_back = match dbchannel_tx_local
                    .unbounded_send(trades)
                    .map_err(|e| {
                        AssetHandlingError::SendingToDatabase(DatabaseChannelError::Trades(e))
                    })
                    .and_then(|_| {
                        order_db_tx_local
                            .unbounded_send(processed_order)
                            .map_err(|e| {
                                AssetHandlingError::SendingToDatabase(DatabaseChannelError::Orders(
                                    e,
                                ))
                            })
                    }) {
                    Ok(_) => os_tx.send(Ok(own_executed_trades)),
                    Err(e) => os_tx.send(Err(e)),
                };

                if let Err(on_db_channel_fail) = sent_back {
                    match on_db_channel_fail {
                        Ok(_) => Err(AssetHandlingError::PhantomError(())),
                        Err(_e) => Err(AssetHandlingError::RouterHasShutDown),
                    }
                } else {
                    Ok(())
                }
            });

        threadpool.spawn(handle_asset.map_err(drop));
    }

    let connections = Arc::new(RwLock::new(addr_maps));

    let srv = socket
        .incoming()
        .map_err(|_e| {
            // TODO: Include in logs.
            panic!("Error accepting socket");
        })
        .map(move |stream| {
            let addr = stream.peer_addr().unwrap();
            let conn_for_client = connections.clone();
            let order_id_local = order_id.clone();

            // TODO: Client authentification
            accept_async(stream)
                .map_err(|e| println!("Handshake failed: {:#?}", e))
                .and_then(move |ws_stream| {
                    println!("New connection from {}", addr);
                    let (response_to_client, order_stream) = ws_stream.split();
                    let (tx, rx) = sync::mpsc::unbounded();

                    let handle_msgs = order_stream
                        .map_err(ServerError::WebSocketError)
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
                        .and_then(move |typed_reader| {
                            let order_reader = match typed_reader.get() {
                                Ok(r) => r,
                                Err(e) => return Err(ServerError::DeserializationError(e)),
                            };

                            let asset_id = order_reader.get_assetname();

                            let ordercondition = match order_reader.get_condition().which() {
                                Ok(ordermsg_capnp::order_msg::condition::Which::Unconditional(
                                    (),
                                )) => order::OrderCondition::Unconditional,
                                Ok(ordermsg_capnp::order_msg::condition::Which::Stoporder(val)) => {
                                    order::OrderCondition::Stop {
                                        stop: val.get_stop(),
                                    }
                                }
                                _ => order::OrderCondition::Unconditional,
                            };

                            let limitprice = match order_reader.get_limitprice().which() {
                                Ok(ordermsg_capnp::order_msg::limitprice::Which::None(())) => None,
                                Ok(ordermsg_capnp::order_msg::limitprice::Which::Some(val)) => {
                                    Some(val)
                                }
                                _ => None,
                            };

                            let order_to_process = order::Order::new(
                                order_id_local.fetch_add(1, atomic::Ordering::SeqCst),
                                order_reader.get_buy(),
                                order_reader.get_volume(),
                                limitprice,
                                ordercondition,
                            );
                            println!("going in: {:#?}", order_to_process);
                            Ok((order_to_process, asset_id))
                        })
                        .and_then(move |(order_to_process, asset_id)| {
                            let (tx_os, rx_os) = futures::sync::oneshot::channel::<
                                std::result::Result<
                                    std::vec::Vec<order::Order>,
                                    AssetHandlingError,
                                >,
                            >();
                            let tx_local = conn_for_client
                                .read()
                                .unwrap()
                                .get(&asset_id)
                                .unwrap()
                                .clone();
                            tx_local
                                .send((order_to_process, tx_os))
                                .map_err(ServerError::PipelineError)
                                .and_then(move |_| rx_os.map_err(ServerError::OneshotCancelled))
                        })
                        .and_then(move |reply| {
                            println!("At the end: {:#?}", reply);
                            //TODO: Write response protocol.
                            match reply {
                                Ok(_) => tx
                                    .unbounded_send(tungstenite::Message::text("And back!"))
                                    .map_err(ServerError::ResponseError),
                                Err(x) => Err(ServerError::AssetHandlingError(x)),
                            }
                        })
                        .for_each(|_| Ok(()));

                    handle_msgs
                        .or_else(|e| {
                            eprintln!("{:#?}", e);
                            Ok(())
                        }) // TODO: handle_msgs fails as expected if client just 'drops' connection. Handle case!
                        .join(
                            response_to_client.send_all(rx.map_err(|_e| {
                                eprintln!("{:#?}", _e);
                                std::io::Error::from(std::io::ErrorKind::Other)
                            })), //.map_err(drop), //TODO: figure out case in which no response handler exists!
                        )
                        .and_then(move |(_, _)| {
                            println!("Connection {} closed.", addr);
                            Ok(())
                        })
                        .or_else(|_e| Ok(())) //TODO: Logging this!
                })
        })
        .buffer_unordered(1000)
        .for_each(|_| Ok(()));

    // Execute server.
    threadpool.spawn(srv);
    threadpool.shutdown_on_idle().wait().unwrap();
}
