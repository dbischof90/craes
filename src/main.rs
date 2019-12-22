
#[cfg(test)]
mod unit_tests;
mod database;
mod errors;
//mod order;
//mod orderbook;
mod order_new;
mod orderbook_new;
mod exchange;
mod protocol_capnp {
    include!(concat!(env!("OUT_DIR"), "/protocol_capnp.rs"));
}

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "CRAES", about = "A centralized exchange implementation.")]
pub struct Config {
    /// Address the server will listen on. 
    #[structopt(long, default_value = "127.0.0.1")]
    server_addr: String,

    /// Server port.
    #[structopt(long, default_value = "8080")]
    server_port: u16,

    /// Postgres DB server address. Must be valid IPv4 address.
    #[structopt(long, default_value = "127.0.0.1")]
    database_addr: String,

    /// Database user name.
    #[structopt(long, default_value = "postgres")]
    database_user: String,

    /// Database name.
    #[structopt(long, default_value = "craes")]
    database_name: String,

    /// Database port.
    #[structopt(long, default_value = "5432")]
    database_port: u16,
}

fn main() {
    let config = Config::from_args();
    let _ = exchange::operate_exchange(config);
}
