use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::engines::bn256::Fs;
use libzeropool::POOL_PARAMS;
use libzeropool::{fawkes_crypto::ff_uint::Num, native::params::PoolBN256};
use libzkbob_rs::{
    client::{state::State, UserAccount as NativeUserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};
use std::fmt::Display;
use std::str::FromStr;
use std::sync::Mutex;
use uuid::Uuid;

use super::tx_parser::StateUpdate;

pub enum DataType {
    Tree,
    Transactions,
    History,
    AccountData,
}

impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataType::Tree => write!(f, "tree"),
            DataType::History => write!(f, "history"),
            DataType::AccountData => write!(f, "account"),
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

        let account_db = kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 1,
                ..Default::default()
            },
            &data_file_path(id, DataType::AccountData)
        )
        .unwrap();

        account_db.write(
            {
                let mut tx = account_db.transaction();
                tx.put(0, "sk".as_bytes(), &dummy_sk);
                tx.put(0, "description".as_bytes(), description.as_bytes());
                tx
            }
        ).unwrap();

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

    pub fn load(account_id: &str) -> Result<Self, String> {
        let id = uuid::Uuid::from_str(account_id).map_err(|err| err.to_string())?;
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

        let account_db = kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 1,
                ..Default::default()
            },
            &data_file_path(id, DataType::AccountData)
        )
        .unwrap();

        let sk = account_db.get(0, "sk".as_bytes()).unwrap().unwrap();
        let description = String::from_utf8(
            account_db.get(0, "description".as_bytes()).unwrap().unwrap()
        ).unwrap();
        
        let user_account = NativeUserAccount::from_seed(&sk, state, POOL_PARAMS.clone());
        Ok(Self {
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
        })
    }
}
