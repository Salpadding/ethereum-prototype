use super::transaction::SignedTransaction;
use crate::{
    ethcore::{transaction_root, Block, Header, MemAccount},
    transaction::TransactionInfo,
};
use crate::{rpc::CHAIN_ID, rpc_types::RpcReceipt};
use ethereum_types::{Bloom, H160, H256, U256, U64};
use evm::backend::{Apply, ApplyBackend, Backend, Basic, Log};
use evm::executor::{MemoryStackState, StackExecutor, StackSubstateMetadata};
use evm::{Capture, Config, Context, CreateScheme, ExitReason, ExitSucceed, Handler};

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Get unix epoch seconds
pub fn unix() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    }
}

pub struct HashMapEthDB {
    pub blocks: Vec<Block>,
    pub accounts: Vec<HashMap<H160, MemAccount>>,
    pub hash_index: HashMap<H256, u64>,
    pub config: Config,
    pub infos: HashMap<H256, TransactionInfo>,
}

pub struct EthDBBackend<'a> {
    pub accounts: &'a mut HashMap<H160, MemAccount>,
    pub gas_price: U256,
    pub sender: H160,
    pub block: &'a Block,
    pub info: &'a mut TransactionInfo,
    pub config: &'a Config,
}

impl<'a> ApplyBackend for EthDBBackend<'a> {
    fn apply<A, I, L>(&mut self, values: A, logs: L, delete_empty: bool)
    where
        A: IntoIterator<Item = Apply<I>>,
        I: IntoIterator<Item = (H256, H256)>,
        L: IntoIterator<Item = Log>,
    {
        for apply in values {
            match apply {
                Apply::Modify {
                    address,
                    basic,
                    code,
                    storage,
                    reset_storage,
                } => {
                    let is_empty = {
                        let account = self.accounts.entry(address).or_insert(Default::default());
                        account.balance = basic.balance;
                        account.nonce = basic.nonce;
                        if let Some(code) = code {
                            account.code = code;
                        }

                        if reset_storage {
                            account.storage = BTreeMap::new();
                        }

                        let zeros = account
                            .storage
                            .iter()
                            .filter(|(_, v)| v == &&H256::default())
                            .map(|(k, _)| k.clone())
                            .collect::<Vec<H256>>();

                        for zero in zeros {
                            account.storage.remove(&zero);
                        }

                        for (index, value) in storage {
                            if value == H256::default() {
                                account.storage.remove(&index);
                            } else {
                                account.storage.insert(index, value);
                            }
                        }

                        account.balance == U256::zero()
                            && account.nonce == U256::zero()
                            && account.code.len() == 0
                    };

                    if is_empty && delete_empty {
                        self.accounts.remove(&address);
                    }
                }
                Apply::Delete { address } => {
                    self.accounts.remove(&address);
                }
            }
        }

        for log in logs {
            self.info.logs.push(log)
        }
    }
}
impl<'a> Backend for EthDBBackend<'a> {
    fn gas_price(&self) -> U256 {
        self.gas_price
    }

    fn origin(&self) -> H160 {
        self.sender
    }

    fn block_hash(&self, number: U256) -> H256 {
        H256::zero()
    }

    fn block_number(&self) -> U256 {
        U256::from(self.block.header.number)
    }

    fn block_coinbase(&self) -> H160 {
        self.block.header.author
    }

    fn block_timestamp(&self) -> U256 {
        U256::from(self.block.header.timestamp)
    }

    fn block_difficulty(&self) -> U256 {
        self.block.header.difficulty
    }

    fn block_gas_limit(&self) -> U256 {
        self.block.header.gas_limit
    }

    fn chain_id(&self) -> U256 {
        U256::from(CHAIN_ID)
    }

    fn exists(&self, address: H160) -> bool {
        self.accounts.contains_key(&address)
    }

    fn basic(&self, address: H160) -> evm::backend::Basic {
        match self.accounts.get(&address) {
            Some(a) => Basic {
                balance: a.balance,
                nonce: a.nonce,
            },
            None => Basic {
                balance: U256::zero(),
                nonce: U256::zero(),
            },
        }
    }

    fn code(&self, address: H160) -> Vec<u8> {
        match self.accounts.get(&address) {
            Some(a) => a.code.clone(),
            None => Vec::new(),
        }
    }

    fn storage(&self, address: H160, index: H256) -> H256 {
        self.accounts
            .get(&address)
            .and_then(|x| x.storage.get(&index).cloned())
            .unwrap_or(H256::zero())
    }

    fn original_storage(&self, address: H160, index: H256) -> Option<H256> {
        self.accounts
            .get(&address)
            .and_then(|x| x.storage.get(&index).cloned())
    }
}

impl<'a> EthDBBackend<'a> {
    pub fn call(
        &mut self,
        caller: H160,
        contract_address: H160,
        input: Vec<u8>,
    ) -> (ExitReason, Vec<u8>, u64) {
        let metadata = StackSubstateMetadata::new(u64::max_value(), &self.config);
        let state = MemoryStackState::new(metadata, self);
        let mut executor = StackExecutor::new(state, &self.config);
        let cap = executor.call(
            contract_address,
            None,
            input,
            Some(u64::MAX),
            true,
            Context {
                address: contract_address,
                caller: caller,
                apparent_value: U256::zero(),
            },
        );

        match cap {
            Capture::Exit((reason, v)) => (reason, v, executor.used_gas()),
            Capture::Trap(int) => (
                ExitReason::Succeed(ExitSucceed::Returned),
                Vec::new(),
                executor.used_gas(),
            ),
        }
    }

    pub fn execute(
        &mut self,
        sender: H160,
        value: U256,
        to: Option<H160>,
        data: Vec<u8>,
    ) -> (ExitReason, Option<Vec<u8>>, u64) {
        // apply transaction
        let metadata = StackSubstateMetadata::new(u64::max_value(), self.config);
        let state = MemoryStackState::new(metadata, self);
        let mut executor = StackExecutor::new(state, self.config);

        let (r, res) = match to {
            Some(h) => {
                let (reason, res) = executor.transact_call(sender, h, value, data, u64::MAX);
                (reason, Some(res))
            }
            None => (
                executor.transact_create(sender, value, data, u64::MAX),
                None,
            ),
        };

        let gas = executor.used_gas();

        match r {
            ExitReason::Succeed(_) => {
                let refer = executor.into_state();
                let (x, y) = refer.deconstruct();
                self.apply(x, y, false);
            }
            _ => {}
        };
        (r, res, gas)
    }
}

impl HashMapEthDB {
    pub fn new() -> HashMapEthDB {
        let mut ret = HashMapEthDB {
            blocks: Vec::new(),
            hash_index: HashMap::new(),
            accounts: Vec::new(),
            config: Config::istanbul(),
            infos: HashMap::new(),
        };

        ret.create_genesis();
        ret
    }

    pub fn submit_tx(&mut self, tx: SignedTransaction) -> Result<(), &'static str> {
        let rc = Arc::new(tx.clone());
        let new_block = self.new_block(vec![rc.clone()]);
        let mut new_accounts = self.accounts.last().unwrap().clone();
        let mut info = TransactionInfo::default();

        let last = self.blocks.last().unwrap();
        info.post_state = last.header.state_root;
        info.parent_hash = last.header.hash();
        info.block_hash = new_block.header.hash();
        info.index = 0;
        info.transaction = Some(rc.clone());

        // create backend for transaction execution
        let mut backend = EthDBBackend {
            accounts: &mut new_accounts,
            sender: tx.sender(),
            gas_price: tx.gas_price,
            block: &new_block,
            info: &mut info,
            config: &self.config,
        };

        let (exit_reason, res, _) =
            backend.execute(tx.sender(), tx.value, tx.receiver(), tx.data.clone());

        match exit_reason {
            ExitReason::Succeed(_) => {
                // insert new block and new states
                println!("submit tx succeed");
                info.result = res;
                self.accounts.push(new_accounts);
                self.save_block(new_block);
                self.infos.insert(tx.hash(), info);
                return Ok(());
            }
            _ => return Err("invalid transaction received"),
        }
    }

    // create a new block
    fn new_block(&self, txs: Vec<Arc<SignedTransaction>>) -> Block {
        let best = self.blocks.last().unwrap();

        let mut h = Header::default();
        h.parent_hash = best.header.hash();
        h.number = best.header.number + 1;
        h.state_root = H256::from_low_u64_be(h.number);
        h.receipts_root = H256::from_low_u64_be(h.number);
        h.timestamp = unix();

        let mut b = Block {
            header: h,
            transactions: txs,
            uncles: Vec::new(),
        };

        b.header.transactions_root = transaction_root(&b.transactions);
        b.header.compute_hash();
        b
    }

    // estimgate gas
    pub fn estimate_gas(
        &mut self,
        block_number: u64,
        caller: H160,
        value: U256,
        to: Option<H160>,
        input: Vec<u8>,
    ) -> (ExitReason, Vec<u8>, u64) {
        let mut info = TransactionInfo::default();

        let mut backend = EthDBBackend {
            accounts: self.accounts.get_mut(block_number as usize).unwrap(),
            sender: caller,
            gas_price: U256::zero(),
            block: self.blocks.last().unwrap(),
            info: &mut info,
            config: &self.config,
        };

        let (r, o, g) = backend.execute(caller, value, to, input);
        (r, o.unwrap_or_default(), g)
    }

    // static call
    pub fn call(
        &mut self,
        block_number: u64,
        caller: H160,
        contract_address: H160,
        input: Vec<u8>,
    ) -> (ExitReason, Vec<u8>, u64) {
        let mut info = TransactionInfo::default();

        let mut backend = EthDBBackend {
            accounts: self.accounts.get_mut(block_number as usize).unwrap(),
            sender: caller,
            gas_price: U256::zero(),
            block: self.blocks.last().unwrap(),
            info: &mut info,
            config: &self.config,
        };

        backend.call(caller, contract_address, input)
    }

    pub fn save_block(&mut self, block: Block) {
        let (h, n) = (block.header.hash(), block.header.number);
        self.blocks.push(block);
        self.hash_index.insert(h, n);
    }

    pub fn best_number(&self) -> u64 {
        (self.blocks.len() - 1) as u64
    }

    pub fn best_block(&self) -> Block {
        self.blocks.get(self.blocks.len() - 1).unwrap().clone()
    }

    pub fn receipt(&self, hash: H256) -> Option<RpcReceipt> {
        match self.infos.get(&hash) {
            None => None,
            Some(info) => {
                let mut r = RpcReceipt::default();
                r.transaction_hash = Some(hash);
                r.transaction_index = Some(U256::from(info.index));
                r.block_hash = Some(info.block_hash);
                let tx = info.transaction.clone().unwrap();
                r.from = Some(tx.sender());
                r.to = tx.receiver();
                r.block_number = self
                    .hash_index
                    .get(&info.block_hash)
                    .cloned()
                    .map(U256::from);

                r.contract_address = tx.contract_address();
                r.state_root = self
                    .blocks
                    .get(r.block_number.unwrap().as_usize())
                    .map(|x| x.header.state_root);
                r.status_code = Some(U64::from(1));
                Some(r)
            }
        }
    }

    #[inline]
    fn create_genesis(&mut self) {
        let mut h = Header::default();
        h.timestamp = unix();
        let hash = h.compute_hash();

        let g = Block {
            header: h,
            transactions: Vec::new(),
            uncles: Vec::new(),
        };

        self.blocks.push(g);
        self.hash_index.insert(hash, 0);
        self.accounts.push(HashMap::new())
    }
}
