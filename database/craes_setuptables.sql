
DROP SCHEMA public CASCADE;
CREATE SCHEMA public;

CREATE TYPE order_type AS ENUM('limit', 'market', 'stoplimit', 'stopmarket');

-- Set up tables

CREATE TABLE asset_info (
    id integer PRIMARY KEY CHECK (id > 0),
    name text NOT NULL
);

CREATE TABLE orders (
    id integer PRIMARY KEY CHECK (id > 0),
    buy boolean,
    volume integer NOT NULL,
    limit_price real,
    order_type order_type,
    stop_price real,
    created_at timestamp without time zone NOT NULL
);

CREATE TABLE trades (
    id integer NOT NULL,
    matched_id integer NOT NULL,
    volume real NOT NULL,
    limit_price real NOT NULL,
    created_at timestamp without time zone NOT NULL,
    filled_at timestamp without time zone NOT NULL
);

CREATE INDEX CONCURRENTLY id_idx ON trades (id);
CREATE INDEX CONCURRENTLY matched_id_idx ON trades (matched_id);

CREATE TABLE user_info (
    username text,
    passphrase text
);

