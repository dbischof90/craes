
# CRAES 
## A centralized asset exchange implemented in Rust
CRAES stands for _Centralized Rust Asset Exchange Software_ and aims to provide a fully asynchronous, stable and scalable high-performance asset exchange.

## Installation and requirements
A few requirements are assumed present on he build system:
- The project compiles against the current stable branch of `rustc` (**1.40.0**) and it is expected to compile against older versions younger than 1.31.0 too without problems.
- A `capnproto v0.7.0^` installation is required to build. On Linux, this can be installed via your favorite package manager (e.g. `apt install capnproto` on Debian-based systems, `pacman -S capnproto` on Arch-based systems, etc.) or by following the instructions on https://capnproto.org/install.html .
- A PostgreSQL 9.6+ server which CRAES can access.

After setting this up you can clone the repository and build with `cargo`:
```bash
$ git clone https://github.com/dbischof90/craes.git
$ cd craes
$ cargo build
```

CRAES assumes that at least four tables are set up and accessible: 
- **`user_info`**: User credentials for client access are stored here.
-  **`asset_info`**: This table stores information on the assets that are set up and tradable on CRAES. During start-up, CRAES reads this table and launches (empty) order books for each asset found.
- **`orders`**:  Incoming orders are logged here by their order ID and the order type information such as limit price, stop price, etc. are saved here.
- **`trades`**: Every successful trade is logged in this table by the IDs of the corresponding orders. 

Schemas for each table can be found in the file `database/craes_setuptables.sql`. 

## Usage
CRAES provides a CLI:
```
$ ./craes -h
CRAES 0.0.9
A centralized exchange implementation.

USAGE:
    craes [OPTIONS]

FLAGS:
    -h,	--help		Prints help information
    -V,	--version 	Prints version information

OPTIONS:
	--database-addr <database-addr> 	Postgres DB server address. Must be valid IPv4 address. [default: 127.0.0.1]
	--database-name <database-name>		Database name. [default: craes]
	--database-port	<database-port>		Database port. [default: 5432]
	--database-user	<database-user>		Database user name. [default: postgres]
	--server-addr <server-addr>		    Address the server will listen on. [default: 127.0.0.1]
	--server-port <server-port> 		Server port. [default: 8080]
```

CRAES yields an structured and event-based diagnostic log to STDOUT which contains information on connection attempts, trading events and potential important information that helps to debug connection problems of clients during runtime or other unexpected events. Currently there is no option to set the verbosity of the log.

## Clients
CRAES uses the `capnproto` protocol for communication, opening potential client implementations to many implementations such as Rust (naturally), C++, Go, Python, OCaml and many more. A list of currently available compiler implementations can be found at https://capnproto.org/otherlang.html . 
Orders are sent to the exchange as serialized messages over a websocket connection. To accept the handshake, CRAES looks for two additional headers in the HTTP request, namely `User` and `Passphrase`. If those match an entry in the `user_info` database, CRAES accepts the connection request and the client can start submitting orders. As of now, the response protocol is only composed out of a list of executed volumes at certain prices which were triggered by the order.
More verbose communication and an RPC protocol for functions like order book querying are still planned.

## Order types
Currently, six different order types are supported: **Market** and **Limit** order versions of **unconditional** (e.g. the standard, well-known market and limit orders), **stop** and **stop-and-reverse** order types. The stop-type orders are implemented in a **strictly prioritizing** manner: If an order is able to fill multiple orders and a stop order becomes eligible, the trade resolution of the initial order is interrupted and the stop order is prioritized. As a stop order triggers another market or limit order, those can trigger other conditional orders as well.
As of now, only immediate trades executed by unconditional orders are reported back immediately to the clients.
