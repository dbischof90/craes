use crate::{database, errors, order, orderbook, protocol_capnp, Config};

use std::collections::HashMap;
use std::sync::{atomic, Arc, Mutex, RwLock};

use dtoa;
use http;

use futures::{
    future,
    prelude::*,
    sink::Sink,
    stream::{iter_ok, Stream},
    sync,
};

use tokio::net::TcpListener;
use tokio_threadpool::blocking;

use capnp;
use capnp_futures;

use tokio_tungstenite::accept_hdr_async;
use tungstenite::handshake::server::{ErrorResponse, Request};

use tracing;
use tracing::{debug, error, info, span, trace, warn, Level};
use tracing_futures::Instrument;
use tracing_subscriber::fmt;

/// Main routine. Launches and operates exchange.
pub fn operate_exchange(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = fmt::Subscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    let server_addr = [config.server_addr.clone(), config.server_port.to_string()]
        .join(":")
        .parse()
        .expect("Not a valid socket address");
    let database::ExchangeSetup {
        asset_ids,
        auth_pairs,
        max_order_id_at_boot,
    } = database::setup_from_database(&config);
    let socket = TcpListener::bind(&server_addr).expect("Could not bind to socket");

    info!("CRAES started, listening on: {}", server_addr);
    let mut threadpool = tokio::runtime::Runtime::new().unwrap();
    let mut addr_maps = HashMap::new();
    let order_id = Arc::new(atomic::AtomicU32::new(max_order_id_at_boot + 1));

    // Database setup. Maps recorded orders and trades to a writable format and
    // copies the message streams into database.
    let (dbchannel_tx, dbchannel_rx) = sync::mpsc::unbounded();
    let (order_db_tx, order_db_rx) = sync::mpsc::unbounded();

    let order_processing = order_db_rx
        .map(|received_order: order::Order| {
            let mut buf = match received_order {
                order::Order::LimitMarket(order) => {
                    let limit_price_string = match order.limit_price {
                        // TODO: Although not expected, errors should be handled here. Find test case
                        // which could trigger failure!
                        Some(price) => {
                            let mut float_buf = Vec::new();
                            dtoa::write(&mut float_buf, price.into_inner()).expect(
                                "Unexpected floating value while deseralizing for database copy",
                            );
                            String::from_utf8(float_buf).unwrap()
                        }
                        None => "\\N".to_string(),
                    };
                    [
                        order.id.to_string(),
                        order.buy.to_string(),
                        order.volume.to_string(),
                        limit_price_string,
                        "marketlimit".to_string(),
                        "\\N".to_string(),
                        order.created_at.to_rfc3339(),
                    ]
                    .join("\t")
                }
                order::Order::StopLimit(order) => {
                    let limit_price_string = match order.limit_price {
                        Some(price) => {
                            let mut float_buf = Vec::new();
                            dtoa::write(&mut float_buf, price.into_inner()).expect(
                                "Unexpected floating value while deseralizing for database copy",
                            );
                            String::from_utf8(float_buf).unwrap()
                        }
                        None => "\\N".to_string(),
                    };

                    let type_string = match order.condition {
                        order::ConditionalType::StopLoss => "stoploss",
                        order::ConditionalType::StopAndReverse => "sar",
                    };
                    [
                        order.id.to_string(),
                        order.buy.to_string(),
                        order.volume.to_string(),
                        limit_price_string,
                        type_string.to_string(),
                        order.trigger_price.to_string(),
                        order.created_at.to_rfc3339(),
                    ]
                    .join("\t")
                }
            };
            buf.push('\n');
            buf.into_bytes()
        })
        .map_err(errors::DatabaseError::ChannelError);

    let trade_processing = dbchannel_rx
        .map(
            |executed_trades: std::collections::HashMap<
            order::Order,
            std::vec::Vec<order::UnconditionalOrder>,
            >| {
                let rows_iter =
                    executed_trades
                    .into_iter()
                    .flat_map(|(executed_order, matched_orders)| {
                        let executed_order_id = match executed_order {
                            order::Order::LimitMarket(order) => order.id,
                            order::Order::StopLimit(order) => order.id,
                        };
                        matched_orders.into_iter().map(move |next_matched_order| {
                            let mut float_buf = Vec::new();
                            dtoa::write(
                                &mut float_buf,
                                next_matched_order.limit_price.unwrap().into_inner(),
                                )
                                .expect(
                                    "Unexpected floating value while deseralizing trades for database copy",
                                    );

                            let mut buf = [
                                executed_order_id.to_string(),
                                next_matched_order.id.to_string(),
                                next_matched_order.volume.to_string(),
                                String::from_utf8(float_buf).unwrap(),
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
                .map_err(errors::DatabaseError::ChannelError);

    database::copy_into_database(
        "trades".to_string(),
        trade_processing,
        &mut threadpool,
        &config,
    );
    database::copy_into_database(
        "orders".to_string(),
        order_processing,
        &mut threadpool,
        &config,
    );

    // Asset handling streams. Receive converted orders from the main stream, execute trades and
    // organize database records plus responses to the clients.
    for i in asset_ids.into_iter() {
        let dbchannel_tx_local = dbchannel_tx.clone();
        let order_db_tx_local = order_db_tx.clone();
        let (assetchannel_tx, assetchannel_rx) = sync::mpsc::channel::<(
            order::Order,
            sync::oneshot::Sender<
                std::result::Result<
                    std::vec::Vec<order::UnconditionalOrder>,
                    errors::AssetHandlingError,
                >,
            >,
        )>(0);
        addr_maps.insert(i, assetchannel_tx);
        let order_book = Arc::new(Mutex::new(orderbook::Orderbook::new(i.to_string())));

        let handle_asset = assetchannel_rx
            .map_err(errors::AssetHandlingError::PhantomError)
            .and_then(move |(order_to_process, os_tx)| {
                let local_orderbook = order_book.clone();
                debug!("Receiving order: {:?}", order_to_process);
                future::poll_fn(move || {
                    blocking(|| {
                        //TODO: Can this or should this be able to fail controlled?
                        local_orderbook
                            .lock()
                            .unwrap()
                            .resolve_order(order_to_process)
                    })
                })
                .map_err(errors::AssetHandlingError::OrderbookInsertError)
                .map(move |trades| (order_to_process, trades, os_tx))
            })
            .for_each(move |(processed_order, trades, os_tx)| {
                let own_executed_trades = match trades.get(&processed_order) {
                    Some(t) => t.clone(),
                    None => std::vec::Vec::new(),
                };
                debug!(
                    "Order triggered {} trades for client.",
                    own_executed_trades.len()
                );

                let sent_back = match dbchannel_tx_local
                    .unbounded_send(trades)
                    .map_err(|e| {
                        errors::AssetHandlingError::SendingToDatabase(
                            errors::DatabaseChannelError::Trades(e),
                        )
                    })
                    .and_then(|_| {
                        order_db_tx_local
                            .unbounded_send(processed_order)
                            .map_err(|e| {
                                errors::AssetHandlingError::SendingToDatabase(
                                    errors::DatabaseChannelError::Orders(e),
                                )
                            })
                    }) {
                    Ok(_) => os_tx.send(Ok(own_executed_trades)),
                    Err(e) => os_tx.send(Err(e)),
                };

                if let Err(on_db_channel_fail) = sent_back {
                    match on_db_channel_fail {
                        Ok(_) => Err(errors::AssetHandlingError::PhantomError(())),
                        Err(_e) => Err(errors::AssetHandlingError::RouterHasShutDown),
                    }
                } else {
                    Ok(())
                }
            })
            .instrument(span!(Level::TRACE, "Order book", "Asset: {}", i));

        threadpool.spawn(handle_asset.map_err(drop));
    }

    let connections = Arc::new(RwLock::new(addr_maps));
    let shared_auth_pairs = Arc::new(RwLock::new(auth_pairs));

    // Main routing server procedure. Organizes client handling, reads messages and converts them into
    // internal order representations which are handled by the asset handlers. After trades are
    // registered, converts and sends information back to clients.
    let srv = socket
        .incoming()
        .map_err(|e| {
            error!("Not able to accept socket: {}", e);
            panic!("Error accepting socket");
        })
        .map(move |stream| {
            let addr = stream.peer_addr().unwrap();
            let conn_for_client = connections.clone();
            let order_id_local = order_id.clone();
            let shared_auth_pairs_local = shared_auth_pairs.clone();

            accept_hdr_async(stream, move |req: &Request| {
                let headers = &req.headers;
                let username = headers.find_first("User");
                let passphrase = headers.find_first("Password");
                let mut base_error = ErrorResponse::from(http::status::StatusCode::BAD_REQUEST);
                let auth_resp = String::from("Response");
                if username.and(passphrase).is_some() {
                    let auth_pairs_unlocked = shared_auth_pairs_local
                        .read()
                        .expect("Problem during auth_pairs unlocking.");

                    let passphrase_entry = auth_pairs_unlocked.get(
                        &String::from_utf8(Vec::from(username.unwrap()))
                            .expect("Corrupted password in database!"),
                    );

                    //TODO: Passwords are checked without hashing. Needs to be implemented!
                    if headers.header_is("Password", passphrase_entry.unwrap_or(&String::from("")))
                    {
                        return Ok(Some(vec![(auth_resp, String::from("Authorized"))]));
                    }
                }
                base_error.headers = Some(vec![(auth_resp, String::from("Wrong credentials"))]);
                error!("Failed connection attempt from {}", &addr);
                Err(base_error)
            })
            .map_err(errors::ServerError::WebSocketError)
            .and_then(move |ws_stream| {
                info!("Client registering at {}", &addr);
                let (response_to_client, order_stream) = ws_stream.split();
                let (tx, rx) = sync::mpsc::unbounded();

                let handle_msgs = order_stream
                    .map_err(errors::ServerError::WebSocketError)
                    .filter_map(|msg| match msg {
                        tungstenite::protocol::Message::Binary(load) => Some(load),
                        _ => None,
                    })
                    .and_then(move |load| {
                        capnp_futures::serialize::read_message(
                            std::io::Cursor::new(load),
                            capnp::message::ReaderOptions::new(),
                        )
                        .map_err(errors::ServerError::DeserializationError)
                    })
                    .filter_map(|(_, root_cont_opt)| root_cont_opt)
                    .map(
                        capnp::message::TypedReader::<
                            capnp_futures::serialize::OwnedSegments,
                            protocol_capnp::order_msg::Owned,
                        >::from,
                    )
                    .and_then(move |typed_reader| {
                        let order_reader = match typed_reader.get() {
                            Ok(r) => r,
                            Err(e) => return Err(errors::ServerError::DeserializationError(e)),
                        };

                        let asset_id = order_reader.get_assetname();
                        let cond_info = match order_reader.get_condition().which() {
                            Ok(protocol_capnp::order_msg::condition::Which::Stoploss(val)) => {
                                Ok(Some((val, order::ConditionalType::StopLoss)))
                            }
                            Ok(protocol_capnp::order_msg::condition::Which::Stopandreverse(
                                val,
                            )) => Ok(Some((val, order::ConditionalType::StopAndReverse))),
                            Err(e) => Err(errors::ServerError::DeserializationError(
                                capnp::Error::from(e),
                            )),
                            _ => Ok(None),
                        };
                        let limit_price = match order_reader.get_limitprice().which() {
                            Ok(protocol_capnp::order_msg::limitprice::Which::Some(val)) => {
                                Ok(Some(val))
                            }
                            Err(e) => Err(errors::ServerError::DeserializationError(
                                capnp::Error::from(e),
                            )),
                            _ => Ok(None),
                        };

                        match (limit_price, cond_info) {
                            (Ok(l_price_opt), Ok(c_info_opt)) => {
                                let new_order_id =
                                    order_id_local.fetch_add(1, atomic::Ordering::SeqCst);
                                let buy_sell = order_reader.get_buy();
                                let volume = order_reader.get_volume();

                                let order_to_process =
                                    if let Some((trigger_price, condition)) = c_info_opt {
                                        order::Order::StopLimit(order::ConditionalOrder::new(
                                            new_order_id,
                                            buy_sell,
                                            volume,
                                            l_price_opt,
                                            condition,
                                            trigger_price,
                                        ))
                                    } else {
                                        order::Order::LimitMarket(order::UnconditionalOrder::new(
                                            new_order_id,
                                            buy_sell,
                                            volume,
                                            l_price_opt,
                                        ))
                                    };

                                Ok((order_to_process, asset_id))
                            }
                            // For simplicity we only expose one price error. Since both
                            // variants are potentially wrong, either option would be fine.
                            (Err(e), _) | (_, Err(e)) => Err(e),
                        }
                    })
                    .and_then(move |(order_to_process, asset_id)| {
                        let (tx_os, rx_os) = futures::sync::oneshot::channel::<
                            std::result::Result<
                                std::vec::Vec<order::UnconditionalOrder>,
                                errors::AssetHandlingError,
                            >,
                        >();

                        // Attempts to find the asset in the map of shared channels and send the
                        // order to the asset handler, expecting an answer.
                        conn_for_client
                            .read()
                            .unwrap()
                            .get(&asset_id)
                            .map(|channel| channel.clone())
                            .ok_or(errors::ServerError::AssetDoesNotExist)
                            .and_then(|tx_local| {
                                Ok(tx_local
                                    .send((order_to_process, tx_os))
                                    .map_err(errors::ServerError::PipelineError)
                                    .and_then(move |_| {
                                        rx_os.map_err(errors::ServerError::OneshotCancelled)
                                    }))
                            })
                    })
                    // Formal double polling required
                    .and_then(|response| response)
                    .and_then(|response| response.map_err(errors::ServerError::AssetHandlingError))
                    .and_then(|trades| {
                        if !trades.is_empty() {
                            trace!("Trades to be sent: {:?}", trades);
                        } else {
                            trace!("No trades executed.");
                        }
                        let mut response_builder = capnp::message::Builder::new_default();
                        let response_msg =
                            response_builder.init_root::<protocol_capnp::response_msg::Builder>();
                        let mut executed_trades_writer =
                            response_msg.init_executedtrades(trades.len() as u32);
                        for (i, order) in trades.iter().enumerate() {
                            let mut volume_traded = executed_trades_writer.reborrow().get(i as u32);
                            volume_traded.set_volume(order.volume);
                            volume_traded.set_price(order.limit_price.unwrap().into_inner());
                        }

                        capnp_futures::serialize::write_message(
                            std::io::Cursor::new(std::vec::Vec::new()),
                            response_builder,
                        )
                        .map_err(errors::ServerError::DeserializationError)
                    })
                    .and_then(move |(buffer, _)| {
                        tx.unbounded_send(tungstenite::Message::binary(buffer.into_inner()))
                            .map_err(errors::ServerError::ResponseError)
                    })
                    .for_each(|_| Ok(()))
                    .instrument(span!(Level::DEBUG, "Order processor", "Client: {}", addr));

                handle_msgs
                    .or_else(|e| {
                        error!("Error in order process: {:?}", e);
                        Ok(())
                    })
                    .join(response_to_client.send_all(rx.map_err(|_| {
                        warn!("No response handler found, dropping task.");
                        std::io::Error::from(std::io::ErrorKind::Other)
                    })))
                    .and_then(move |(_, _)| {
                        info!("Connection to {} closed.", addr);
                        Ok(())
                    })
                    .or_else(move |e| {
                        error!(
                            "Failed response to {}, dropping connection. Reason: {:?}",
                            addr, e
                        );
                        Ok(())
                    })
            })
            .or_else(|e| {
                error!("Handshake failed: {:?}", e);
                Ok(())
            })
        })
        .buffer_unordered(1000)
        .for_each(|_| Ok(()));

    // Execute server.
    threadpool.spawn(srv.map_err(drop));
    threadpool.shutdown_on_idle().wait().unwrap();

    Ok(())
}
