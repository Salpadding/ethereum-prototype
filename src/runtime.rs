use db::HashMapEthDB;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Runtime {
    pub eth_db: Arc<RwLock<HashMapEthDB>>,
}