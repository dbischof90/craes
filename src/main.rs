use std::collections::HashMap;
use std::env;
use std::sync::{Arc, RwLock};

use futures::{prelude::*, sink::Sink, stream::Stream, sync};

use tokio::{
    io::{Error, ErrorKind},
    net::TcpListener,
};

use tokio_tungstenite::accept_async;
use tungstenite::protocol::Message;

fn main() {
    let server_addr = env::args()
        .nth(1)
        .unwrap_or("127.0.0.1:8080".to_string())
        .parse()
        .unwrap();

    // Create the event loop and TCP listener we'll accept connections on.
    let socket = TcpListener::bind(&server_addr).unwrap();
    println!("Listening on: {}", server_addr);

    let mut threadpool = tokio::runtime::Runtime::new().unwrap();
    let mut addr_maps = HashMap::new();

    // Tokio Runtime uses a thread pool based executor by default, so we need
    // to use Arc and RwLock to store the map of all connections we know about.
    for i in 0..3 {
        let (asset_tx, asset_rx) =
            sync::mpsc::channel::<(Message, sync::oneshot::Sender<Message>)>(0);
        addr_maps.insert(i, asset_tx);

        let handle_asset = asset_rx.for_each(|(j, os_tx)| {
            println!("I received {} for asset !", &j);
            os_tx.send(j).unwrap();
            Ok(())
        });

        threadpool.spawn(handle_asset);
    }

    let connections = Arc::new(RwLock::new(addr_maps));

    let srv = socket
        .incoming()
        .map(move |stream| {
            let addr = stream.peer_addr().unwrap();
            let conn_for_client = connections.clone();
            accept_async(stream).map_err(drop).and_then(move |ws_stream| {
                println!("New connection from {}", addr);
                let (response_to_client, order_stream) = ws_stream.split();
                
                let (tx, rx) = sync::mpsc::unbounded();
                let handle_orders = order_stream.map_err(drop)
                    .and_then(move |msg| {
                        let (tx_os, rx_os) = futures::sync::oneshot::channel::<Message>();
                        let tx_local = conn_for_client.read().unwrap().get(&1).unwrap().clone();
                        tx_local
                            .send((msg, tx_os))
                            .map_err(drop)
                            .and_then(move |_| rx_os.map_err(drop))
                    })
                    .map(move |reply| tx.unbounded_send(reply))
                    .for_each(|_| { 
                        Ok(())
                    });

                let back_to_client = rx.forward(response_to_client);

                handle_orders.join(back_to_client).then(move |_| {
                    println!("Connection {} closed.", addr);
                    Ok(())
                })

            })
        })
        .map_err(drop)
        .for_each(|_| Ok(()));
    // Execute server.
    threadpool.spawn(srv);
    threadpool.shutdown_on_idle().wait().unwrap();
}
