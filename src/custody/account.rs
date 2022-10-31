use std::{cell::RefCell, rc::Rc};

use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::engines::bn256::{Fr, Fs};
use libzeropool::{native::params::PoolBN256, fawkes_crypto::ff_uint::Num};
use libzeropool::POOL_PARAMS;
use libzkbob_rs::{
    client::{state::State, UserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};
use uuid::Uuid;

use super::tx_parser::{IndexedTx, TxParser};
pub struct Account {
    pub user_account: Rc<RefCell<UserAccount<kvdb_rocksdb::Database, PoolBN256>>>,
    pub id: Uuid,
    pub description: String,
    pub history: kvdb_rocksdb::Database,
}

impl Account {
    pub fn next_index(&self) -> u64 {
        self.user_account.as_ref().borrow().state.tree.next_index()
    }

    pub fn sk(&self) -> Num<Fs> {
        self.user_account.as_ref().borrow().keys.sk
    }
    pub fn new(description: String) -> Self {
        let id = uuid::Uuid::new_v4();
        let state = State::new(
            MerkleTree::new_native(
                Default::default(),
                format!("{}-tr", id).as_str(),
                POOL_PARAMS.clone(),
            )
            .unwrap(),
            SparseArray::new_native(
                &Default::default(),
                format!("{}-tx", &id.as_hyphenated()).as_str(),
            )
            .unwrap(),
        );

        let user_account = UserAccount::from_seed("seed".as_bytes(), state, POOL_PARAMS.clone());
        Self {
            user_account: Rc::new(RefCell::new(user_account)),
            id,
            description,
            history: kvdb_rocksdb::Database::open(
                &DatabaseConfig {
                    columns: 3,
                    ..Default::default()
                },
                format!("{}-history", &id.to_string()).as_str(),
            )
            .unwrap(),
        }
    }

    pub fn sync(&mut self, txs: Vec<IndexedTx>) {
        let tx_parser = TxParser::new();

        let sk = self.user_account.borrow().keys.sk;
        let parse_result = tx_parser.parse_native_tx(sk, txs);
    }
}
