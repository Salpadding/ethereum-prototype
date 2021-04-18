extern crate ethereum_types;
extern crate evm;
extern crate hex;
extern crate jsonrpc_core;
extern crate jsonrpc_derive;
extern crate jsonrpc_http_server;
#[macro_use]
extern crate serde_derive;
extern crate keccak_hash;
extern crate parity_bytes;
extern crate rlp;
extern crate serde;
extern crate serde_json;

extern crate parity_crypto;

use std::sync::{atomic::AtomicUsize, Arc, RwLock};

use jsonrpc_core::{IoHandler, MetaIoHandler};
use rpc::{Eth, Net, Web3};

mod db;
mod ethcore;
mod middleware;
mod rpc;
mod rpc_types;
mod runtime;
mod transaction;

fn main() {
    let mut db = db::HashMapEthDB::new();
    let r = runtime::Runtime {
        eth_db: Arc::new(RwLock::new(db)),
    };

    let mut handler = MetaIoHandler::with_middleware(middleware::MyMiddleware(0.into()));
    let socket_addr: std::net::SocketAddr = "0.0.0.0:8545".parse().unwrap();
    let w3 = rpc::Web3Client;
    let eth = rpc::EthClient::new(r);
    let n = rpc::NetClient;

    handler.extend_with(n.to_delegate());
    handler.extend_with(w3.to_delegate());
    handler.extend_with(eth.to_delegate());

    let server = rpc::start_http(&socket_addr, handler).unwrap();
    server.wait();
}
