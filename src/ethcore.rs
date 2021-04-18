use super::transaction::{SignedTransaction, UnverifiedTransaction};
use ethereum_types::{BigEndianHash, Bloom, H160, H256, H512, U256};
use keccak_hash::keccak;
use parity_bytes::Bytes;
use parity_crypto::publickey::{public_to_address, Public};
use rlp::{DecoderError, Encodable, Rlp, RlpStream};
use std::sync::Arc;
use std::{collections::BTreeMap, ops::Deref};

pub fn transaction_root<T: Deref<Target = SignedTransaction>>(txs: &[T]) -> H256 {
    let mut s = RlpStream::new();
    s.begin_list(txs.len());

    for t in txs.iter() {
        let h = t.hash();
        s.append(&h);
    }

    keccak(s.out())
}

#[derive(Default, Clone, Debug, Eq, PartialEq)]
pub struct MemAccount {
    /// Account nonce.
    pub nonce: U256,
    /// Account balance.
    pub balance: U256,
    /// Full account storage.
    pub storage: BTreeMap<H256, H256>,
    /// Account code.
    pub code: Vec<u8>,
}

/// Transaction action type.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Create creates new contract.
    Create,
    /// Calls contract at given address.
    /// In the case of a transfer, this is the receiver's address.'
    Call(H160),
}

impl Default for Action {
    fn default() -> Action {
        Action::Create
    }
}

impl rlp::Encodable for Action {
    fn rlp_append(&self, s: &mut RlpStream) {
        match *self {
            Action::Create => s.append_internal(&""),
            Action::Call(ref addr) => s.append_internal(addr),
        };
    }
}

impl rlp::Decodable for Action {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.is_empty() {
            if rlp.is_data() {
                Ok(Action::Create)
            } else {
                Err(DecoderError::RlpExpectedToBeData)
            }
        } else {
            Ok(Action::Call(rlp.as_val()?))
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Block {
    /// The header of this block.
    pub header: Header,
    /// The transactions in this block.
    pub transactions: Vec<Arc<SignedTransaction>>,
    /// The uncles of this block.
    pub uncles: Vec<Header>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// Parent hash.
    pub parent_hash: H256,
    /// Block timestamp.
    pub timestamp: u64,
    /// Block number.
    pub number: u64,
    /// Block author.
    pub author: H160,

    /// Transactions root.
    pub transactions_root: H256,
    /// Block uncles hash.
    pub uncles_hash: H256,
    /// Block extra data.
    pub extra_data: Bytes,

    /// State root.
    pub state_root: H256,
    /// Block receipts root.
    pub receipts_root: H256,
    /// Block bloom.
    pub log_bloom: Bloom,
    /// Gas used for contracts execution.
    pub gas_used: U256,
    /// Block gas limit.
    pub gas_limit: U256,

    /// Block difficulty.
    pub difficulty: U256,

    /// Memoized hash of that header and the seal.
    pub hash: Option<H256>,
}

impl Default for Header {
    fn default() -> Header {
        Header {
            parent_hash: H256::zero(),
            timestamp: 0,
            number: 0,
            author: H160::zero(),

            transactions_root: keccak_hash::KECCAK_NULL_RLP,
            uncles_hash: keccak_hash::KECCAK_EMPTY_LIST_RLP,
            extra_data: vec![],

            state_root: keccak_hash::KECCAK_NULL_RLP,
            receipts_root: keccak_hash::KECCAK_NULL_RLP,
            log_bloom: Bloom::default(),
            gas_used: U256::default(),
            gas_limit: U256::default(),

            difficulty: U256::default(),
            hash: None,
        }
    }
}

impl Header {
    fn rlp(&self) -> Vec<u8> {
        let mut s = RlpStream::new();
        self.stream_rlp(&mut s);
        s.out().to_vec()
    }

    /// Place this header into an RLP stream `s`, optionally `with_seal`.
    fn stream_rlp(&self, s: &mut RlpStream) {
        s.begin_list(13);
        s.append(&self.parent_hash);
        s.append(&self.uncles_hash);
        s.append(&self.author);
        s.append(&self.state_root);
        s.append(&self.transactions_root);
        s.append(&self.receipts_root);
        s.append(&self.log_bloom);
        s.append(&self.difficulty);
        s.append(&self.number);
        s.append(&self.gas_limit);
        s.append(&self.gas_used);
        s.append(&self.timestamp);
        s.append(&self.extra_data);
    }

    /// Get & memoize the hash of this header (keccak of the RLP with seal).
    pub fn compute_hash(&mut self) -> H256 {
        let hash = self.hash();
        self.hash = Some(hash);
        hash
    }

    /// Get the hash of this header (keccak of the RLP with seal).
    pub fn hash(&self) -> H256 {
        self.hash.unwrap_or_else(|| keccak(self.rlp()))
    }
}

impl Encodable for Header {
    fn rlp_append(&self, s: &mut RlpStream) {
        self.stream_rlp(s);
    }
}
