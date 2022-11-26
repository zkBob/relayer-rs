use ethabi::ethereum_types::{U64, U256};
use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::engines::bn256::Fs;
use libzeropool::fawkes_crypto::ff_uint::NumRepr;
use libzeropool::fawkes_crypto::rand::Rng;
use libzeropool::POOL_PARAMS;
use libzeropool::{fawkes_crypto::ff_uint::Num, native::params::PoolBN256};
use libzkbob_rs::address;
use libzkbob_rs::random::CustomRng;
use libzkbob_rs::{
    client::{state::State, UserAccount as NativeUserAccount},
    merkle::MerkleTree,
    sparse_array::SparseArray,
};
use libzeropool::native::account::Account as NativeAccount;
use memo_parser::memo::TxType;
use memo_parser::memoparser;
use tokio::sync::RwLock;
use std::fmt::Display;
use std::str::FromStr;

use uuid::Uuid;

use crate::contracts::Pool;
use crate::custody::types::{HistoryTx, PoolParams};

use super::errors::CustodyServiceError;
use super::tx_parser::StateUpdate;
use super::types::{HistoryDbColumn, HistoryRecord, HistoryTxType, AccountShortInfo, Fr};

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

    last_task_key: RwLock<Option<Vec<u8>>>
}

impl Account {
    pub async fn get_tx_parts(&self, total_amount: u64, fee: u64, to: String) -> Result<Vec<(Option<String>, Num<Fr>)>, CustodyServiceError> {
        let account = self.inner.read().await;
        let amount = Num::from_uint(NumRepr::from(total_amount)).unwrap();
        let fee = Num::from_uint(NumRepr::from(fee)).unwrap();

        let mut account_balance = account.state.account_balance();
        let mut parts = vec![];

        if account_balance.to_uint() >= (amount + fee).to_uint() {
            parts.push((Some(to), amount));
            return Ok(parts)
        }
        
        let notes = account.state.get_usable_notes();
        let mut balance_is_sufficient = false;
        for notes in notes.chunks(3) {
            let mut note_balance = Num::ZERO;
            for (_, note) in notes {
                note_balance += note.b.as_num();
            }

            if (note_balance + account_balance).to_uint() >= (amount + fee).to_uint() {
                parts.push((Some(to), amount));
                balance_is_sufficient = true;
                break;
            } else {
                parts.push((None, note_balance - fee));
                account_balance += note_balance - fee;
            }
        }

        if !balance_is_sufficient {
            return Err(CustodyServiceError::InsufficientBalance);
        }

        Ok(parts)
    }

    pub async fn short_info(&self) -> AccountShortInfo {
        let inner = self.inner.read().await;
        AccountShortInfo {
            id: self.id.to_string(),
            description: self.description.clone(),
            balance: inner.state.total_balance().to_string(),
        }
    }

    pub async fn update_state(&self, state_update: StateUpdate) {
        let mut inner = self.inner.write().await;
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

    pub async fn next_index(&self) -> u64 {
        let inner = self.inner.read().await;
        inner.state.tree.next_index()
    }

    pub async fn sk(&self) -> Num<Fs> {
        let inner = self.inner.read().await;
        inner.keys.sk
    }

    pub async fn generate_address(&self) -> String {
        let inner = self.inner.read().await;
        inner.generate_address()
    }

    pub async fn history<F>(&self, get_transaction_id: F, pool: Option<&Pool>) -> Vec<HistoryTx>
    where
        F: Fn(Vec<u8>) -> Result<String, String>,
    {
        let mut history = vec![];
        let mut last_account: Option<NativeAccount<Fr>> = None;
        for (_, value) in self.history.iter(HistoryDbColumn::NotesIndex.into()) {
            let tx: HistoryRecord = serde_json::from_slice(&value).unwrap();
            let calldata = memoparser::parse_calldata(&tx.calldata, None).unwrap();
            let dec_memo = tx.dec_memo;
            let tx_type = TxType::from_u32(calldata.tx_type);
            let nullifier = calldata.nullifier;

            let history_records = match tx_type {
                TxType::Deposit => vec![(
                    HistoryTxType::Deposit,
                    calldata.token_amount.to_string(),
                    None,
                )],
                TxType::DepositPermittable => vec![(
                    HistoryTxType::Deposit,
                    calldata.token_amount.to_string(),
                    None,
                )],
                TxType::Transfer => {
                    let mut history_records = vec![];
                    
                    if dec_memo.in_notes.len() == 0 && dec_memo.out_notes.len() == 0 {
                        let amount = {
                            let previous_amount = match last_account {
                                Some(acc) => acc.b.as_num().clone(),
                                None => Num::ZERO,
                            };
                            dec_memo.acc.unwrap().b.as_num() - previous_amount
                        };
                        
                        history_records.push((
                            HistoryTxType::AggregateNotes,
                            amount.to_string(),
                            None
                        ))
                    }
                    
                    for note in dec_memo.in_notes.iter() {
                        let loopback = dec_memo.out_notes
                            .iter()
                            .find(|out_note| out_note.index == note.index)
                            .is_some();

                        let tx_type = if loopback { HistoryTxType::ReturnedChange } else { HistoryTxType::TransferIn };
                        let address = address::format_address::<PoolParams>(note.note.d, note.note.p_d);

                        history_records.push((
                            tx_type,
                            note.note.b.to_num().to_string(),
                            Some(address),
                        ))
                    }

                    let out_notes = dec_memo.out_notes
                        .iter()
                        .filter(|out_note| {
                            dec_memo.in_notes.iter().find(|in_note| in_note.index == out_note.index).is_none()
                        });
                    for note in out_notes {
                        let address = address::format_address::<PoolParams>(note.note.d, note.note.p_d);
                        history_records.push((
                            HistoryTxType::TransferOut,
                            note.note.b.to_num().to_string(),
                            Some(address),
                        ))
                    }
                    history_records
                }
                TxType::Withdrawal => vec![(
                    HistoryTxType::Withdrawal,
                    (-(calldata.memo.fee as i128 + calldata.token_amount)).to_string(),
                    None,
                )],
            };

            let transaction_id = get_transaction_id(nullifier).ok();

            let timestamp = match pool {
                Some(pool) => self.block_timestamp(pool, tx.block_num).await,
                None => Default::default(),
            };

            for (tx_type, amount, to) in history_records {
                history.push(HistoryTx {
                    tx_hash: format!("{:#x}", tx.tx_hash),
                    amount,
                    timestamp: timestamp.to_string(),
                    tx_type,
                    to,
                    transaction_id: transaction_id.clone(),
                })
            }

            if let Some(acc) = dec_memo.acc {
                last_account = Some(acc);
            }
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
            &data_file_path(base_path, id, DataType::AccountData),
        )
        .unwrap();

        account_db
            .write({
                let mut tx = account_db.transaction();
                tx.put(0, "sk".as_bytes(), &sk);
                tx.put(0, "description".as_bytes(), description.as_bytes());
                tx
            })
            .unwrap();

        let user_account = NativeUserAccount::from_seed(&sk, state, POOL_PARAMS.clone());
        Self {
            inner: RwLock::new(user_account),
            id,
            description,
            history: kvdb_rocksdb::Database::open(
                &DatabaseConfig {
                    columns: 2,
                    ..Default::default()
                },
                &data_file_path(base_path, id, DataType::History),
            )
            .unwrap(),
            last_task_key: RwLock::new(None)
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
            &data_file_path(base_path, id, DataType::AccountData),
        )
        .unwrap();

        let sk = account_db.get(0, "sk".as_bytes()).unwrap().unwrap();
        let description = String::from_utf8(
            account_db
                .get(0, "description".as_bytes())
                .unwrap()
                .unwrap(),
        )
        .unwrap();

        let user_account = NativeUserAccount::from_seed(&sk, state, POOL_PARAMS.clone());
        Ok(Self {
            inner: RwLock::new(user_account),
            id,
            description,
            history: kvdb_rocksdb::Database::open(
                &DatabaseConfig {
                    columns: 2,
                    ..Default::default()
                },
                &data_file_path(base_path, id, DataType::History),
            )
            .unwrap(),
            last_task_key: RwLock::new(None)
        })
    }

    pub async fn update_last_task(&self, task_key: Vec<u8>) {
        let mut last_task = self.last_task_key.write().await;
        *last_task = Some(task_key);
    }

    pub async fn last_task(&self) -> Option<Vec<u8>> {
        let last_task_key = self.last_task_key.read().await;
        last_task_key.clone()
    }

    async fn block_timestamp(&self, pool: &Pool, block_number: U64) -> U256 {
        let timestamp = self.history
            .get(
                HistoryDbColumn::BlockTimestampsCache.into(), 
                &block_number.as_u64().to_be_bytes()
            )
            .ok()
            .flatten()
            .map(|bytes| {
                U256::from_big_endian(&bytes)
            });

        match timestamp {
            Some(timestamp) => timestamp,
            None => {
                let timestamp = pool.block_timestamp(block_number).await;
                match timestamp {
                    Ok(timestamp) => {
                        self.history.write({
                            let mut timestamp_bytes = [0; 32];
                            timestamp.to_big_endian(&mut timestamp_bytes);
                            
                            let mut tx = self.history.transaction();
                            tx.put(
                                HistoryDbColumn::BlockTimestampsCache.into(), 
                                &block_number.as_u64().to_be_bytes(), 
                                &timestamp_bytes
                            );
                            tx
                        }).unwrap();
                        timestamp
                    }
                    Err(_) => Default::default()
                }
            }
        }
    }
}
