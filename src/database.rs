use std::iter::FromIterator;

use crate::{errors, Config};

use tracing;
use tracing::{debug, error, span, Level};
use tracing_futures::Instrument;

use futures::{
    prelude::*,
    stream::{iter_ok, Stream},
};

#[derive(Debug)]
pub struct ExchangeSetup {
    pub asset_ids: std::vec::Vec<u16>,
    pub auth_pairs: std::collections::HashMap<String, String>,
    pub max_order_id_at_boot: u32,
}

pub fn copy_into_database<S>(
    table_name: String,
    lines_stream: S,
    threadpool: &mut tokio::runtime::Runtime,
    config: &Config,
) where
    S: Stream<Item = std::vec::Vec<u8>, Error = errors::DatabaseError> + Send + 'static,
{
    let executor_handle = threadpool.executor();
    let table_name_cl = table_name.clone();
    let db_config = format!(
        "host={db_host} user={db_user} dbname={db_name} port={db_port}",
        db_host = config.database_addr,
        db_user = config.database_user,
        db_name = config.database_name,
        db_port = config.database_port
    );

    let fut = tokio_postgres::connect(
        &db_config,
        tokio_postgres::NoTls,
    )
    .map(move |(client, connection)| {
        executor_handle.spawn(connection.map_err(|e| error!("Connection error: {:?}", e)));
        client
    })
    .and_then(move |mut client| {
        client
            .prepare(&["COPY ", &table_name, " FROM STDIN"].concat())
            .map(|statement| (client, statement))
    })
    .map_err(errors::DatabaseError::ClientError)
    .and_then(|(mut client, statement)| {
        lines_stream
            .chunks(2)
            .and_then(move |prepared_rows| {
                debug!("Writing {} lines", &prepared_rows.len());
                let stream_rows = iter_ok::<_, std::io::Error>(prepared_rows);
                client
                    .copy_in(&statement, &[], stream_rows)
                    .map_err(errors::DatabaseError::ClientError)
            })
            .or_else(|e| {
                error!("Error while copying to database: {:?}", e);
                Ok(0u64)
            })
            .for_each(|_| Ok(()))
    })
    .instrument(span!(
        Level::TRACE,
        "Database handler",
        "Table: {}",
        table_name_cl
    ));

    threadpool.spawn(fut.map_err(drop));
}

pub fn setup_from_database(config: &Config) -> ExchangeSetup {
    let mut temporary_runtime = tokio::runtime::Runtime::new().unwrap();
    let executor_handle = temporary_runtime.executor();

    let db_config = format!(
        "host={db_host} user={db_user} dbname={db_name} port={db_port}",
        db_host = config.database_addr,
        db_user = config.database_user,
        db_name = config.database_name,
        db_port = config.database_port
    );

    let fut = tokio_postgres::connect(
        &db_config,
        tokio_postgres::NoTls,
    )
    .map(move |(client, connection)| {
        executor_handle.spawn(connection.map_err(|e| error!("Connection error: {:?}", e)));
        client
    })
    .and_then(move |mut client| {
        let asset_id_prep = client.prepare("SELECT id FROM asset_info");
        let auth_pairs_prep = client.prepare("SELECT username, passphrase FROM user_info");
        let max_order_id_prep = client.prepare("SELECT COALESCE(MAX(id), 0) AS id FROM orders");
        asset_id_prep.join3(auth_pairs_prep, max_order_id_prep).map(
            |(asset_id_prep, auth_pairs_prep, max_order_id_prep)| {
                (client, asset_id_prep, auth_pairs_prep, max_order_id_prep)
            },
        )
    })
    .map_err(errors::DatabaseError::ClientError)
    .and_then(
        |(mut client, asset_id_stmt, auth_pairs_stmt, max_order_id_stmt)| {
            let asset_id_res = client
                .query(&asset_id_stmt, &[])
                .collect()
                .map(|rows| rows.into_iter().map(|row| row.get::<_, i32>(0) as u16))
                .map_err(errors::DatabaseError::ClientError);

            let auth_pairs_res = client
                .query(&auth_pairs_stmt, &[])
                .collect()
                .map(|rows| {
                    rows.into_iter()
                        .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                })
                .map_err(errors::DatabaseError::ClientError);

            let max_order_id_res = client
                .query(&max_order_id_stmt, &[])
                .collect()
                .map(|rows| rows[0].get::<_, i32>(0) as u32)
                .map_err(errors::DatabaseError::ClientError);

            (asset_id_res, auth_pairs_res, max_order_id_res)
        },
    );
    let (asset_id, auth_pairs, max_order_id) = temporary_runtime
        .block_on(fut)
        .expect("Problem during setup download from database.");

    ExchangeSetup {
        asset_ids: asset_id.collect(),
        auth_pairs: std::collections::HashMap::from_iter(auth_pairs),
        max_order_id_at_boot: max_order_id,
    }
}
