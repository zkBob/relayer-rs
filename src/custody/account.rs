use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::engines::bn256::{Fr, Fs};
use libzeropool::POOL_PARAMS;
use libzeropool::{fawkes_crypto::ff_uint::Num, native::params::PoolBN256};
use libzkbob_rs::{
    client::{state::State, UserAccount as NativeUserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};
use std::fmt::Display;
use std::io::Write;
use std::sync::Mutex;
use uuid::Uuid;

use super::tx_parser::{IndexedTx, StateUpdate, TxParser};

pub enum DataType {
    Tree,
    Transactions,
    History,
    SecretKey,
}

impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataType::Tree => write!(f, "tree"),
            DataType::History => write!(f, "history"),
            DataType::SecretKey => write!(f, "sk"),
            DataType::Transactions => write!(f, "tx"),
        }
    }
}

pub fn data_file_path(account_id: Uuid, data_type: DataType) -> String {
    let data_root = "accounts_data";

    format!("{}/{}/{}", data_root, account_id.as_hyphenated(), data_type)
}
pub struct Account {
    pub inner: Mutex<NativeUserAccount<kvdb_rocksdb::Database, PoolBN256>>,
    pub id: Uuid,
    pub description: String,
    pub history: kvdb_rocksdb::Database,
}

impl Account {
    pub fn update_state(&self, state_update: StateUpdate) {
        let mut inner = self.inner.lock().unwrap();
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
                &data_file_path(id, DataType::Tree),
                POOL_PARAMS.clone(),
            )
            .unwrap(),
            SparseArray::new_native(
                &Default::default(),
                &data_file_path(id, DataType::Transactions),
            )
            .unwrap(),
        );

        //"increase dial aisle hedgehog tree genre replace deliver boat tower furnace image"
        let dummy_sk =
            hex::decode("e0f3b09c27af2986df5c4157dc54e7b43d73748ccf2568e2ea21f2037b887800")
                .unwrap();

        let mut sk_file = std::fs::File::create(data_file_path(id, DataType::SecretKey)).unwrap();
        sk_file.write_all(&dummy_sk).unwrap();

        let user_account = NativeUserAccount::from_seed(&dummy_sk, state, POOL_PARAMS.clone());
        Self {
            inner: Mutex::new(user_account),
            id,
            description,
            history: kvdb_rocksdb::Database::open(
                &DatabaseConfig {
                    columns: 3,
                    ..Default::default()
                },
                &data_file_path(id, DataType::History)
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
