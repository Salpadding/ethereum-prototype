use super::rpc_types::{CallRequest, Request, Rich, RichBlock, RpcBlock};
use crate::{
    db::HashMapEthDB, runtime, transaction::SignedTransaction, transaction::UnverifiedTransaction,
};
use ethereum_types::{Bloom, H160, H256, U256, U64};
use evm::ExitReason;
use jsonrpc_core::{Error as RpcError, ErrorCode, Result as RpcResult};
use jsonrpc_derive::rpc;
use jsonrpc_http_server as http;
use keccak_hash::keccak;
use rpc_types::{Bytes, RpcLog, RpcReceipt};
use serde::de::{Error, MapAccess, Visitor};
use std::fmt;
use std::{collections::BTreeMap, ops::Deref};
use std::{net::SocketAddr, str::FromStr};

macro_rules! get_db {
    ($runtime: expr) => {{
        let mut e = jsonrpc_core::types::Error::internal_error();
        e.message = "failed to require runtime.eth_db read lock".into();
        $runtime.eth_db.read().map_err(|_| e)
    };};
}

macro_rules! get_mut_db {
    ($runtime: expr) => {{
        let mut e = jsonrpc_core::types::Error::internal_error();
        e.message = "failed to require runtime.eth_db read lock".into();
        $runtime.eth_db.write().map_err(|_| e)
    };};
}

macro_rules! rpc_err {
    ($msg: expr) => {{
        let mut e = jsonrpc_core::types::Error::internal_error();
        e.message = $msg.into();
        Err(e)
    }};
}

macro_rules! rpc_try {
    ($result: expr, $msg: expr) => {{
        let mut e = jsonrpc_core::types::Error::internal_error();
        e.message = $msg.into();
        $result.map_err(|_| e)
    }};
}

pub const CHAIN_ID: u64 = 127;

#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub enum BlockNumber {
    /// Hash
    Hash {
        /// block hash
        hash: H256,
        /// only return blocks part of the canon chain
        require_canonical: bool,
    },
    /// Number
    Num(u64),
    /// Latest block
    Latest,
    /// Earliest block (genesis)
    Earliest,
    /// Pending block (being mined)
    Pending,
}

impl Default for BlockNumber {
    fn default() -> Self {
        BlockNumber::Latest
    }
}

struct BlockNumberVisitor;

impl<'a> serde::Deserialize<'a> for BlockNumber {
    fn deserialize<D>(deserializer: D) -> Result<BlockNumber, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        deserializer.deserialize_any(BlockNumberVisitor)
    }
}

impl<'a> Visitor<'a> for BlockNumberVisitor {
    type Value = BlockNumber;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "a block number or 'latest', 'earliest' or 'pending'"
        )
    }

    fn visit_map<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
    where
        V: MapAccess<'a>,
    {
        let (mut require_canonical, mut block_number, mut block_hash) =
            (false, None::<u64>, None::<H256>);

        loop {
            let key_str: Option<String> = visitor.next_key()?;

            match key_str {
                Some(key) => match key.as_str() {
                    "blockNumber" => {
                        let value: String = visitor.next_value()?;
                        if value.starts_with("0x") {
                            let number = u64::from_str_radix(&value[2..], 16).map_err(|e| {
                                Error::custom(format!("Invalid block number: {}", e))
                            })?;

                            block_number = Some(number);
                            break;
                        } else {
                            return Err(Error::custom(
                                "Invalid block number: missing 0x prefix".to_string(),
                            ));
                        }
                    }
                    "blockHash" => {
                        block_hash = Some(visitor.next_value()?);
                    }
                    "requireCanonical" => {
                        require_canonical = visitor.next_value()?;
                    }
                    key => return Err(Error::custom(format!("Unknown key: {}", key))),
                },
                None => break,
            };
        }

        if let Some(number) = block_number {
            return Ok(BlockNumber::Num(number));
        }

        if let Some(hash) = block_hash {
            return Ok(BlockNumber::Hash {
                hash,
                require_canonical,
            });
        }

        return Err(Error::custom("Invalid input"));
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        match value {
            "latest" => Ok(BlockNumber::Latest),
            "earliest" => Ok(BlockNumber::Earliest),
            "pending" => Ok(BlockNumber::Pending),
            _ if value.starts_with("0x") => u64::from_str_radix(&value[2..], 16)
                .map(BlockNumber::Num)
                .map_err(|e| Error::custom(format!("Invalid block number: {}", e))),
            _ => Err(Error::custom(
                "Invalid block number: missing 0x prefix".to_string(),
            )),
        }
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(value.as_ref())
    }
}

pub fn start_http<M, S, H>(
    addr: &SocketAddr,
    handler: H,
) -> ::std::io::Result<jsonrpc_http_server::Server>
where
    M: jsonrpc_core::Metadata + Default,
    S: jsonrpc_core::Middleware<M>,
    H: Into<jsonrpc_core::MetaIoHandler<M, S>>,
{
    Ok(http::ServerBuilder::new(handler)
        .keep_alive(true)
        .threads(4)
        .cors(http::DomainsValidation::Disabled)
        .allowed_hosts(http::DomainsValidation::Disabled)
        .health_api(("/api/health", "parity_nodeStatus"))
        .cors_allow_headers(http::cors::AccessControlAllowHeaders::Any)
        .max_request_body_size(64 * 1024 * 1024)
        .start_http(addr)?)
}

#[rpc(server)]
pub trait Web3 {
    /// Returns current client version.
    #[rpc(name = "web3_clientVersion")]
    fn client_version(&self) -> RpcResult<String>;

    /// Returns sha3 of the given data
    #[rpc(name = "web3_sha3")]
    fn sha3(&self, data: Bytes) -> RpcResult<H256>;
}

pub struct Web3Client;

impl Web3 for Web3Client {
    fn client_version(&self) -> RpcResult<String> {
        Ok("tethereum v0.1; rust".to_string())
    }

    fn sha3(&self, data: Bytes) -> RpcResult<H256> {
        Ok(keccak(data.0))
    }
}

/// Net rpc interface.
#[rpc(server)]
pub trait Net {
    /// Returns protocol version.
    #[rpc(name = "net_version")]
    fn version(&self) -> RpcResult<String>;

    /// Returns number of peers connected to node.
    #[rpc(name = "net_peerCount")]
    fn peer_count(&self) -> RpcResult<U64>;

    /// Returns true if client is actively listening for network connections.
    /// Otherwise false.
    #[rpc(name = "net_listening")]
    fn is_listening(&self) -> RpcResult<bool>;
}

pub struct NetClient;

impl Net for NetClient {
    fn version(&self) -> RpcResult<String> {
        Ok(CHAIN_ID.to_string())
    }

    fn peer_count(&self) -> RpcResult<U64> {
        Ok(U64::zero())
    }

    fn is_listening(&self) -> RpcResult<bool> {
        Ok(false)
    }
}

/// Eth rpc interface.
#[rpc(server)]
pub trait Eth {
    /// Returns protocol version encoded as a string (quotes are necessary).
    #[rpc(name = "eth_protocolVersion")]
    fn protocol_version(&self) -> RpcResult<String>;

    /// Returns block author.
    #[rpc(name = "eth_coinbase")]
    fn author(&self) -> RpcResult<H160>;

    /// Returns an object with data about the sync status or false. (wtf?)
    #[rpc(name = "eth_syncing")]
    fn syncing(&self) -> RpcResult<bool>;

    /// Returns true if client is actively mining new blocks.
    #[rpc(name = "eth_mining")]
    fn is_mining(&self) -> RpcResult<bool>;

    /// Returns the chain ID used for transaction signing at the
    /// current best block. None is returned if not
    /// available.
    #[rpc(name = "eth_chainId")]
    fn chain_id(&self) -> RpcResult<Option<U64>>;

    /// Returns current gas_price.
    #[rpc(name = "eth_gasPrice")]
    fn gas_price(&self) -> RpcResult<U256>;

    /// Returns accounts list.
    #[rpc(name = "eth_accounts")]
    fn accounts(&self) -> RpcResult<Vec<H160>>;

    /// Returns highest block number.
    #[rpc(name = "eth_blockNumber")]
    fn block_number(&self) -> RpcResult<U256>;

    /// Sends signed transaction, returning its hash.
    #[rpc(name = "eth_sendRawTransaction")]
    fn send_raw_transaction(&self, _: Bytes) -> RpcResult<H256>;

    /// Call contract, returning the output data.
    #[rpc(name = "eth_call")]
    fn call(&self, _: CallRequest, _: Option<BlockNumber>) -> RpcResult<Bytes>;

    /// Returns the number of hashes per second that the node is mining with.
    #[rpc(name = "eth_hashrate")]
    fn hashrate(&self) -> RpcResult<U256>;

    /// Returns balance of the given account.
    #[rpc(name = "eth_getBalance")]
    fn balance(&self, _: H160, _: Option<BlockNumber>) -> RpcResult<U256>;

    /// Returns block with given number.
    #[rpc(name = "eth_getBlockByNumber")]
    fn block_by_number(&self, _: BlockNumber, _: bool) -> RpcResult<Option<RichBlock>>;

    /// Estimate gas needed for execution of given contract.
    #[rpc(name = "eth_estimateGas")]
    fn estimate_gas(&self, _: CallRequest, _: Option<BlockNumber>) -> RpcResult<U256>;

    /// Returns the number of transactions sent from given address at given time (block number).
    #[rpc(name = "eth_getTransactionCount")]
    fn transaction_count(&self, _: H160, _: Option<BlockNumber>) -> RpcResult<U256>;

    /// Returns transaction receipt by transaction hash.
    #[rpc(name = "eth_getTransactionReceipt")]
    fn transaction_receipt(&self, _: H256) -> RpcResult<Option<RpcReceipt>>;
}

pub struct EthClient {
    runtime: runtime::Runtime,
}

impl EthClient {
    pub fn new(r: runtime::Runtime) -> EthClient {
        EthClient { runtime: r }
    }

    fn parse_number<T: Deref<Target = HashMapEthDB>>(
        num: Option<BlockNumber>,
        db: T,
    ) -> Result<u64, &'static str> {
        let num = num.unwrap_or(BlockNumber::Latest);
        let number = match num {
            BlockNumber::Earliest => 0,
            BlockNumber::Hash {
                hash,
                require_canonical,
            } => db.hash_index.get(&hash).cloned().ok_or("err")?,
            BlockNumber::Num(n) => n,
            BlockNumber::Pending => db.best_number(),
            BlockNumber::Latest => db.best_number(),
        };

        Ok(number)
    }
}

impl Eth for EthClient {
    fn protocol_version(&self) -> RpcResult<String> {
        Ok("tethereum".to_string())
    }

    fn author(&self) -> RpcResult<H160> {
        Ok(H160::zero())
    }

    fn is_mining(&self) -> RpcResult<bool> {
        Ok(true)
    }

    fn chain_id(&self) -> RpcResult<Option<U64>> {
        let u: U64 = CHAIN_ID.into();
        Ok(Some(u))
    }

    fn gas_price(&self) -> RpcResult<U256> {
        Ok(U256::zero())
    }

    fn accounts(&self) -> RpcResult<Vec<H160>> {
        Ok(vec![])
    }

    fn block_number(&self) -> RpcResult<U256> {
        let db = get_db!(self.runtime)?;
        Ok(U256::from(db.best_number()))
    }

    fn send_raw_transaction(&self, raw: Bytes) -> RpcResult<H256> {
        // parse unverified transaction from rlp
        let ele = rlp::Rlp::new(&raw.0);
        let tx: UnverifiedTransaction = rpc_try!(
            ele.as_val(),
            "rpc error: parse raw transaction failed from rlp"
        )?;

        let signed = rpc_try!(SignedTransaction::new(tx), "verify transaction failed")?;

        let h = signed.hash();
        println!("send_raw_transaction {} received", h);

        // TODO: verify signatrue
        let mut db = get_mut_db!(self.runtime)?;
        rpc_try!(
            db.submit_tx(signed).map(|_| h),
            "rpc error: invalid transaction received"
        )
    }

    fn call(&self, req: CallRequest, num: Option<BlockNumber>) -> RpcResult<Bytes> {
        let to = rpc_try!(req.to.ok_or(""), "missing contract address")?;
        let from = req.from.unwrap_or(H160::zero());
        let db = get_db!(self.runtime)?;
        let n = rpc_try!(EthClient::parse_number(num, db), "parse number failed")?;

        let mut db = get_mut_db!(self.runtime)?;

        let (reason, ret, _) = db.call(n, from, to, req.data.map(|x| x.0).unwrap_or(Vec::new()));

        match reason {
            ExitReason::Succeed(_) => Ok(Bytes(ret)),
            _ => rpc_err!("call failed"),
        }
    }

    fn hashrate(&self) -> RpcResult<U256> {
        Ok(U256::zero())
    }

    fn syncing(&self) -> RpcResult<bool> {
        Ok(false)
    }

    fn balance(&self, address: H160, num: Option<BlockNumber>) -> RpcResult<U256> {
        let db = get_db!(self.runtime)?;
        let number = rpc_try!(
            EthClient::parse_number(num, db),
            "parse block number failed"
        )?;

        let db = get_db!(self.runtime)?;
        let states = rpc_try!(
            db.accounts.get(number as usize).ok_or(""),
            "invalid block number"
        )?;

        let balance = states
            .get(&address)
            .map(|a| a.balance)
            .unwrap_or(U256::zero());

        Ok(balance)
    }

    fn block_by_number(&self, _: BlockNumber, _: bool) -> RpcResult<Option<RichBlock>> {
        let db = get_db!(self.runtime)?;
        let best = db.best_block();
        let r: RpcBlock = (&best).into();

        Ok(Some(Rich {
            inner: r,
            extra_info: BTreeMap::new(),
        }))
    }

    fn estimate_gas(&self, req: CallRequest, num: Option<BlockNumber>) -> RpcResult<U256> {
        let from = req.from.unwrap_or(H160::zero());
        let db = get_db!(self.runtime)?;
        let n = rpc_try!(EthClient::parse_number(num, db), "parse number failed")?;

        let mut db = get_mut_db!(self.runtime)?;

        let (_, _, gas) = db.estimate_gas(
            n,
            from,
            req.value.unwrap_or_default(),
            req.to,
            req.data.map(|x| x.0).unwrap_or(Vec::new()),
        );
        Ok(U256::from(gas))
    }

    fn transaction_count(&self, k: H160, num: Option<BlockNumber>) -> RpcResult<U256> {
        let db = get_db!(self.runtime)?;
        let n = rpc_try!(EthClient::parse_number(num, db), "parse number failed")?;
        let db = get_db!(self.runtime)?;

        let nonce = db
            .accounts
            .get(n as usize)
            .and_then(|x| x.get(&k))
            .map(|x| x.nonce)
            .unwrap_or(U256::zero());

        Ok(nonce)
    }

    fn transaction_receipt(&self, h: H256) -> RpcResult<Option<RpcReceipt>> {
        println!("transaction_receipt: hash = {}", h);
        let db = get_db!(self.runtime)?;
        Ok(db.receipt(h))
    }
}
