use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::engines::bn256::Fs;
use libzeropool::POOL_PARAMS;
use libzeropool::fawkes_crypto::rand::Rng;
use libzeropool::{fawkes_crypto::ff_uint::Num, native::params::PoolBN256};
use libzkbob_rs::address;
use libzkbob_rs::random::CustomRng;
use libzkbob_rs::{
    client::{state::State, UserAccount as NativeUserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};
use memo_parser::memo::TxType;
use memo_parser::memoparser;
use std::fmt::Display;
use std::str::FromStr;
use std::sync::RwLock;
use uuid::Uuid;

use crate::contracts::Pool;
use crate::custody::types::{HistoryTx, PoolParams};

use super::tx_parser::StateUpdate;
use super::types::{HistoryRecord, HistoryDbColumn, HistoryTxType};

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

pub fn data_file_path(base_path: &str, account_id: Uuid, data_type: DataType) -> String {
    format!("{}/{}/{}", base_path, account_id.as_hyphenated(), data_type)
}
pub struct Account {
    pub inner: RwLock<NativeUserAccount<kvdb_rocksdb::Database, PoolBN256>>,
    pub id: Uuid,
    pub description: String,
    pub history: kvdb_rocksdb::Database,
}

impl Account {
    pub fn update_state(&self, state_update: StateUpdate) {
        let mut inner = self.inner.write().unwrap();
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
        let inner = self.inner.read().unwrap();
        inner.state.tree.next_index()
    }

    pub fn sk(&self) -> Num<Fs> {
        let inner = self.inner.read().unwrap();
        inner.keys.sk
    }

    pub async fn history<F>(&self, pool: &Pool, get_transaction_id: F) -> Vec<HistoryTx> 
        where F: Fn(Vec<u8>) -> Result<String, String> 
    {
        let mut history = vec![];
        for (_, value) in self.history.iter(HistoryDbColumn::NotesIndex.into()) {
            let tx: HistoryRecord = serde_json::from_slice(&value).unwrap();
            let calldata = memoparser::parse_calldata(&tx.calldata, None).unwrap();
            let dec_memo = tx.dec_memo;
            let tx_type = TxType::from_u32(calldata.tx_type);
            let nullifier = calldata.nullifier;
            
            let (tx_type, amount, to) = match tx_type {
                TxType::Deposit => {
                    (HistoryTxType::Deposit, calldata.token_amount.to_string(), None)
                },
                TxType::DepositPermittable => {
                    (HistoryTxType::Deposit, calldata.token_amount.to_string(), None)
                },
                TxType::Transfer => {
                    if dec_memo.in_notes.len() > 0 {
                        if dec_memo.out_notes.len() > 0 {
                            // TODO: multitransfer
                            let note = &dec_memo.out_notes[0].note;
                            let amount = note.b.to_num();
                            let address = address::format_address::<PoolParams>(note.d, note.p_d);
                            (HistoryTxType::TransferLoopback, amount.to_string(), Some(address))
                        } else {
                            // TODO: multitransfer
                            let note = &dec_memo.in_notes[0].note;
                            let amount = note.b.to_num();
                            let address = address::format_address::<PoolParams>(note.d, note.p_d);
                            (HistoryTxType::TransferIn, amount.to_string(), Some(address))
                        }
                    } else {
                        // TODO: multitransfer
                        let note = &dec_memo.out_notes[0].note;
                        let amount = note.b.to_num();
                        let address = address::format_address::<PoolParams>(note.d, note.p_d);
                        (HistoryTxType::TransferOut, amount.to_string(), Some(address))
                    }
                },
                TxType::Withdrawal => {
                    (HistoryTxType::Withdrawal, (-(calldata.memo.fee as i128 + calldata.token_amount)).to_string(), None)
                }
            };

            let transaction_id = get_transaction_id(nullifier).ok();
            
            let timestamp = pool.block_timestamp(tx.block_num).await.unwrap();
            history.push(HistoryTx {
                tx_hash: format!("{:#x}", tx.tx_hash),
                tx_index: calldata.tx_index.to_string(),
                amount,
                timestamp: timestamp.to_string(),
                tx_type,
                to,
                transaction_id
            })
        }
        history
    }

    pub fn new(base_path: &str, description: String) -> Self {    
        let id = uuid::Uuid::new_v4();
        let state = State::new(
            MerkleTree::new_native(
                Default::default(),
                &data_file_path(base_path, id, DataType::Tree),
                POOL_PARAMS.clone(),
            )
            .unwrap(),
            SparseArray::new_native(
                &Default::default(),
                &data_file_path(base_path, id, DataType::Transactions),
            )
            .unwrap(),
        );

        let mut rng = CustomRng;
        let sk: [u8; 32] = rng.gen();

        let account_db = kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 1,
                ..Default::default()
            },
            &data_file_path(base_path, id, DataType::AccountData)
        )
        .unwrap();

        account_db.write(
            {
                let mut tx = account_db.transaction();
                tx.put(0, "sk".as_bytes(), &sk);
                tx.put(0, "description".as_bytes(), description.as_bytes());
                tx
            }
        ).unwrap();

        let user_account = NativeUserAccount::from_seed(&sk, state, POOL_PARAMS.clone());
        Self {
            inner: RwLock::new(user_account),
            id,
            description,
            history: kvdb_rocksdb::Database::open(
                &DatabaseConfig {
                    columns: 1,
                    ..Default::default()
                },
                &data_file_path(base_path, id, DataType::History)
            )
            .unwrap(),
        }
    }

    pub fn load(base_path: &str, account_id: &str) -> Result<Self, String> {
        let id = uuid::Uuid::from_str(account_id).map_err(|err| err.to_string())?;
        let state = State::new(
            MerkleTree::new_native(
                Default::default(),
                &data_file_path(base_path, id, DataType::Tree),
                POOL_PARAMS.clone(),
            )
            .unwrap(),
            SparseArray::new_native(
                &Default::default(),
                &data_file_path(base_path, id, DataType::Transactions),
            )
            .unwrap(),
        );

        let account_db = kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 1,
                ..Default::default()
            },
            &data_file_path(base_path, id, DataType::AccountData)
        )
        .unwrap();

        let sk = account_db.get(0, "sk".as_bytes()).unwrap().unwrap();
        let description = String::from_utf8(
            account_db.get(0, "description".as_bytes()).unwrap().unwrap()
        ).unwrap();
        
        let user_account = NativeUserAccount::from_seed(&sk, state, POOL_PARAMS.clone());
        Ok(Self {
            inner: RwLock::new(user_account),
            id,
            description,
            history: kvdb_rocksdb::Database::open(
                &DatabaseConfig {
                    columns: 1,
                    ..Default::default()
                },
                &data_file_path(base_path, id, DataType::History)
            )
            .unwrap(),
        })
    }
}
