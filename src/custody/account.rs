
use libzeropool::native::params::PoolBN256;
use libzeropool::POOL_PARAMS;
use libzkbob_rs::{
    client::{state::State, UserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};

use super::tx_parser::{TxParser, IndexedTx};
pub struct Account {
    pub user_account: UserAccount<kvdb_rocksdb::Database, PoolBN256>,
    pub id: String,
    pub description: String,
}

impl Account {
    pub fn new(description: String) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let state = State::new(
            MerkleTree::new_native(
                Default::default(),
                format!("{}-tr", id).as_str(),
                POOL_PARAMS.clone(),
            )
            .unwrap(),
            SparseArray::new_native(&Default::default(), format!("{}-tx", id).as_str()).unwrap(),
        );

        let user_account = UserAccount::from_seed("seed".as_bytes(), state, POOL_PARAMS.clone());
        Self {
            user_account,
            id,
            description,
        }
    }

    pub fn sync(& mut self, txs: Vec<IndexedTx>) {
        let tx_parser = TxParser::new();


        let parse_result = tx_parser.parse_native_tx(self.user_account.keys.sk, txs);
    }
}

