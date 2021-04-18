use super::hex;
use crate::ethcore::{Block, Header};
use crate::transaction::UnverifiedTransaction;
use ethereum_types::{Bloom, H160, H256, H512, U256, U64};
use rlp::Encodable;
use serde::de::{Error, Visitor};
use serde::ser::Error as SerError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;

/// Value representation with additional info
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rich<T> {
    /// Standard value.
    pub inner: T,
    /// Engine-specific fields with additional description.
    /// Should be included directly to serialized block object.
    // TODO [ToDr] #[serde(skip_serializing)]
    pub extra_info: BTreeMap<String, String>,
}

impl<T> Deref for Rich<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Block representation with additional info.
pub type RichBlock = Rich<RpcBlock>;

impl<T: Serialize> Serialize for Rich<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde_json::{to_value, Value};

        let serialized = (to_value(&self.inner), to_value(&self.extra_info));
        if let (Ok(Value::Object(mut value)), Ok(Value::Object(extras))) = serialized {
            // join two objects
            value.extend(extras);
            // and serialize
            value.serialize(serializer)
        } else {
            Err(S::Error::custom(
                "Unserializable structures: expected objects",
            ))
        }
    }
}

/// Block Transactions
#[derive(Debug)]
pub enum BlockTransactions {
    /// Only hashes
    Hashes(Vec<H256>),
    /// Full transactions
    Full(Vec<RpcTransaction>),
}

impl Serialize for BlockTransactions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match *self {
            BlockTransactions::Hashes(ref hashes) => hashes.serialize(serializer),
            BlockTransactions::Full(ref ts) => ts.serialize(serializer),
        }
    }
}

/// Represents condition on minimum block number or block timestamp.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TransactionCondition {
    /// Valid at this minimum block number.
    #[serde(rename = "block")]
    Number(u64),
    /// Valid at given unix time.
    #[serde(rename = "time")]
    Timestamp(u64),
}

/// Transaction
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransaction {
    /// Hash
    pub hash: H256,
    /// Nonce
    pub nonce: U256,
    /// Block hash
    pub block_hash: Option<H256>,
    /// Block number
    pub block_number: Option<U256>,
    /// Transaction Index
    pub transaction_index: Option<U256>,
    /// Sender
    pub from: H160,
    /// Recipient
    pub to: Option<H160>,
    /// Transfered value
    pub value: U256,
    /// Gas Price
    pub gas_price: U256,
    /// Gas
    pub gas: U256,
    /// Data
    pub input: Bytes,
    /// Creates contract
    pub creates: Option<H160>,
    /// Raw transaction data
    pub raw: Bytes,
    /// Public key of the signer.
    pub public_key: Option<H512>,
    /// The network id of the transaction, if any.
    pub chain_id: Option<U64>,
    /// The standardised V field of the signature (0 or 1).
    pub standard_v: U256,
    /// The standardised V field of the signature.
    pub v: U256,
    /// The R field of the signature.
    pub r: U256,
    /// The S field of the signature.
    pub s: U256,
    /// Transaction activates at specified block.
    pub condition: Option<TransactionCondition>,
}

/// Block representation
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlock {
    /// Hash of the block
    pub hash: Option<H256>,
    /// Hash of the parent
    pub parent_hash: H256,
    /// Hash of the uncles
    #[serde(rename = "sha3Uncles")]
    pub uncles_hash: H256,
    /// Authors address
    pub author: H160,
    /// Alias of `author`
    pub miner: H160,
    /// State root hash
    pub state_root: H256,
    /// Transactions root hash
    pub transactions_root: H256,
    /// Transactions receipts root hash
    pub receipts_root: H256,
    /// Block number
    pub number: Option<U256>,
    /// Gas Used
    pub gas_used: U256,
    /// Gas Limit
    pub gas_limit: U256,
    /// Extra data
    pub extra_data: Bytes,
    /// Logs bloom
    pub logs_bloom: Option<Bloom>,
    /// Timestamp
    pub timestamp: U256,
    /// Difficulty
    pub difficulty: U256,
    /// Total difficulty
    pub total_difficulty: Option<U256>,
    /// Seal fields
    pub seal_fields: Vec<Bytes>,
    /// Uncles' hashes
    pub uncles: Vec<H256>,
    /// Transactions
    pub transactions: BlockTransactions,
    /// Size in bytes
    pub size: Option<U256>,
}

/// Request
#[derive(Debug, Default, PartialEq)]
pub struct Request {
    /// From
    pub from: Option<H160>,
    /// To
    pub to: Option<H160>,
    /// Gas Price
    pub gas_price: Option<U256>,
    /// Gas
    pub gas: Option<U256>,
    /// Value
    pub value: Option<U256>,
    /// Data
    pub data: Option<Vec<u8>>,
    /// Nonce
    pub nonce: Option<U256>,
}

/// Call request
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct CallRequest {
    /// From
    pub from: Option<H160>,
    /// To
    pub to: Option<H160>,
    /// Gas Price
    pub gas_price: Option<U256>,
    /// Gas
    pub gas: Option<U256>,
    /// Value
    pub value: Option<U256>,
    /// Data
    pub data: Option<Bytes>,
    /// Nonce
    pub nonce: Option<U256>,
}

impl Into<Request> for CallRequest {
    fn into(self) -> Request {
        Request {
            from: self.from.map(Into::into),
            to: self.to.map(Into::into),
            gas_price: self.gas_price.map(Into::into),
            gas: self.gas.map(Into::into),
            value: self.value.map(Into::into),
            data: self.data.map(Into::into),
            nonce: self.nonce.map(Into::into),
        }
    }
}

/// Wrapper structure around vector of bytes.
#[derive(Debug, PartialEq, Default, Hash, Clone)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    /// Simple constructor.
    pub fn new(bytes: Vec<u8>) -> Bytes {
        Bytes(bytes)
    }
    /// Convert back to vector
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(bytes: Vec<u8>) -> Bytes {
        Bytes(bytes)
    }
}

impl Into<Vec<u8>> for Bytes {
    fn into(self) -> Vec<u8> {
        self.0
    }
}

impl Serialize for Bytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut serialized = "0x".to_owned();
        serialized.push_str(&hex::encode(&self.0));
        serializer.serialize_str(serialized.as_ref())
    }
}

impl<'a> Deserialize<'a> for Bytes {
    fn deserialize<D>(deserializer: D) -> Result<Bytes, D::Error>
    where
        D: Deserializer<'a>,
    {
        deserializer.deserialize_any(BytesVisitor)
    }
}

struct BytesVisitor;

impl<'a> Visitor<'a> for BytesVisitor {
    type Value = Bytes;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a 0x-prefixed, hex-encoded vector of bytes")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        if value.len() >= 2 && value.starts_with("0x") && value.len() & 1 == 0 {
            Ok(Bytes::new(hex::decode(&value[2..]).map_err(|e| {
                Error::custom(format!("Invalid hex: {}", e))
            })?))
        } else {
            Err(Error::custom(
                "Invalid bytes format. Expected a 0x-prefixed hex string with even length",
            ))
        }
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: Error,
    {
        self.visit_str(value.as_ref())
    }
}

impl<'a> From<&'a Header> for RpcBlock {
    fn from(h: &'a Header) -> Self {
        RpcBlock {
            hash: Some(h.hash()),
            parent_hash: h.parent_hash,
            uncles_hash: h.uncles_hash,
            author: h.author,
            miner: h.author,
            state_root: h.state_root,
            transactions_root: h.transactions_root,
            receipts_root: h.receipts_root,
            number: Some(h.number.into()),
            gas_used: h.gas_used,
            extra_data: Bytes(h.extra_data.clone()),
            logs_bloom: Some(h.log_bloom),
            timestamp: h.timestamp.into(),
            difficulty: h.difficulty,
            total_difficulty: None,
            seal_fields: Vec::new(),
            uncles: Vec::new(),
            transactions: BlockTransactions::Full(Vec::new()),
            size: Some(h.rlp_bytes().len().into()),
            gas_limit: h.gas_limit,
        }
    }
}

impl<'a> From<&'a Block> for RpcBlock {
    fn from(o: &'a Block) -> RpcBlock {
        let mut b: RpcBlock = (&o.header).into();
        b
    }
}

/// Log
#[derive(Debug, Serialize, PartialEq, Hash, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RpcLog {
    /// H160
    pub address: H160,
    /// Topics
    pub topics: Vec<H256>,
    /// Data
    pub data: Bytes,
    /// Block Hash
    pub block_hash: Option<H256>,
    /// Block Number
    pub block_number: Option<U256>,
    /// Transaction Hash
    pub transaction_hash: Option<H256>,
    /// Transaction Index
    pub transaction_index: Option<U256>,
    /// Log Index in Block
    pub log_index: Option<U256>,
    /// Log Index in Transaction
    pub transaction_log_index: Option<U256>,
    /// Log Type
    #[serde(rename = "type")]
    pub log_type: String,
    /// Whether Log Type is Removed (Geth Compatibility Field)
    #[serde(default)]
    pub removed: bool,
}

/// Receipt
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RpcReceipt {
    /// Transaction Hash
    pub transaction_hash: Option<H256>,
    /// Transaction index
    pub transaction_index: Option<U256>,
    /// Block hash
    pub block_hash: Option<H256>,
    /// Sender
    pub from: Option<H160>,
    /// Recipient
    pub to: Option<H160>,
    /// Block number
    pub block_number: Option<U256>,
    /// Cumulative gas used
    pub cumulative_gas_used: U256,
    /// Gas used
    pub gas_used: Option<U256>,
    /// Contract address
    pub contract_address: Option<H160>,
    /// Logs
    pub logs: Vec<RpcLog>,
    /// State Root
    // NOTE(niklasad1): EIP98 makes this optional field, if it's missing then skip serializing it
    #[serde(skip_serializing_if = "Option::is_none", rename = "root")]
    pub state_root: Option<H256>,
    /// Logs bloom
    pub logs_bloom: Bloom,
    /// Status code
    // NOTE(niklasad1): Unknown after EIP98 rules, if it's missing then skip serializing it
    #[serde(skip_serializing_if = "Option::is_none", rename = "status")]
    pub status_code: Option<U64>,
}
