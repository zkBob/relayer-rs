use std::{cell::RefCell, sync::Mutex};

use crate::{
    custody::tx_parser::ParseResult,
    helpers,
    routes::ServiceError,
    state::{State, DB},
};
use actix_web::{
    web::{Data, Json},
    HttpResponse,
};
use kvdb::KeyValueDB;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use super::{
    account::Account,
    tx_parser::{self, IndexedTx, TxParser},
};

pub struct CustodyService {
    accounts: Vec<Account>,
}

#[derive(Serialize, Deserialize)]
pub struct SignupRequest {
    description: String,
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let mut custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;
    let new_acc = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().body(new_acc.id.to_string()))
}
pub enum HistoryDbColumn {
    TxHashIndex,
    NotesIndex,
}

impl Into<u32> for HistoryDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}

impl CustodyService {
    pub fn new() -> Self {
        Self { accounts: vec![] }
    }

    pub fn new_account(&mut self, description: String) -> Account {
        let account = Account::new(description);
        self.accounts.push(account);
        account
    }

    pub fn sync_account(
        &self,
        account_id: Uuid,
        relayer_state: Data<State<kvdb_rocksdb::Database>>,
    ) {
        tracing::info!("starting sync for account {}", account_id);
        if let Some(account) = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
        {
            tracing::info!("account {} found", account_id);
            let start_index = account.next_index();

            let finalized = relayer_state.finalized.lock().unwrap();
            let finalized_index = finalized.next_index();
            tracing::info!(
                "account {}, finalized index = {} ",
                account_id,
                finalized_index
            );
            // let batch_size = ??? as u64; //TODO: loop
            let jobs = relayer_state
                .get_jobs(start_index, finalized_index)
                .unwrap();

            let indexed_txs: Vec<IndexedTx> = jobs
                .iter()
                .enumerate()
                .map(|item| IndexedTx {
                    index: item.0 as u64,
                    memo: (item.1).memo.clone(),
                    commitment: (item.1).commitment,
                })
                .collect();

            let job_indices: Vec<u64> = jobs.iter().map(|j| j.index).collect();

            tracing::info!("jobs to sync {:#?}", job_indices);

            let parse_result: ParseResult =
                TxParser::new().parse_native_tx(account.sk(), indexed_txs);

            tracing::info!(
                "new_accounts: {:#?} \n new notes: {:#?}",
                parse_result.state_update.new_accounts,
                parse_result.state_update.new_notes
            );
            let decrypted_memos = parse_result.decrypted_memos;

            let mut batch = account.history.transaction();
            decrypted_memos.iter().for_each(|memo| {
                batch.put_vec(
                    HistoryDbColumn::NotesIndex.into(),
                    &tx_parser::index_key(memo.index),
                    serde_json::to_vec(memo).unwrap(),
                );
            });

            account.history.write(batch).unwrap();

            account.update_state(parse_result.state_update);
        }
        ()
    }
}
