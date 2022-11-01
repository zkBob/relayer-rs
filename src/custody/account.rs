use std::sync::Mutex;
use std::{cell::RefCell, rc::Rc};

use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::engines::bn256::{Fr, Fs};
use libzeropool::POOL_PARAMS;
use libzeropool::{fawkes_crypto::ff_uint::Num, native::params::PoolBN256};
use libzkbob_rs::{
    client::{state::State, UserAccount as NativeUserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};
use uuid::Uuid;

use super::tx_parser::{IndexedTx, StateUpdate, TxParser};
pub struct Account {
    pub inner: Mutex<NativeUserAccount<kvdb_rocksdb::Database, PoolBN256>>,
    pub id: Uuid,
    pub description: String,
    pub history: kvdb_rocksdb::Database,
}

impl Account {
    pub fn update_state(&self, state_update: StateUpdate) {
        let inner = self.inner.lock().unwrap();
        if !state_update.new_leafs.is_empty() || !state_update.new_commitments.is_empty() {
            inner
                .state
                .tree
                .add_leafs_and_commitments(state_update.new_leafs, state_update.new_commitments);
        }

        state_update
            .new_accounts
            .into_iter()
            .for_each(|(at_index, account)| {
                inner.state.add_account(at_index, account);
            });

        state_update.new_notes.into_iter().for_each(|notes| {
            notes.into_iter().for_each(|(at_index, note)| {
                inner.state.add_note(at_index, note);
            });
        });
    }

    pub fn next_index(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.state.tree.next_index()
    }

    pub fn sk(&self) -> Num<Fs> {
        let inner = self.inner.lock().unwrap();
        inner.keys.sk
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

        let user_account = NativeUserAccount::from_seed(
            "increase dial aisle hedgehog tree genre replace deliver boat tower furnace image"
                .as_bytes(),
            state,
            POOL_PARAMS.clone(),
        );
        Self {
            inner: Mutex::new(user_account),
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

    // pub fn sync(&mut self, txs: Vec<IndexedTx>) {
    //     let tx_parser = TxParser::new();

    //     let sk = self.inner.borrow().keys.sk;
    //     let parse_result = tx_parser.parse_native_tx(sk, txs);
    // }
}
