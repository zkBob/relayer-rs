use crate::{
    custody::{
        tx_parser::ParseResult,
        types::{HistoryDbColumn, HistoryRecord},
    },
    state::State,
    types::{
        transaction_request::{Proof, TransactionRequest},
    },
};
use actix_web::web::Data;
use kvdb::KeyValueDB;
use kvdb_rocksdb::{Database, DatabaseConfig};
use libzeropool::fawkes_crypto::ff_uint::NumRepr;
use libzeropool::{
    circuit::tx::c_transfer,
    fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters},
    fawkes_crypto::{backend::bellman_groth16::prover::prove, ff_uint::Num},
};
use memo_parser::memo::TxType as MemoTxType;
use serde::{Deserialize, Serialize};
use tracing_futures::Instrument;
use std::{
    fs, thread,
    time::{Duration, SystemTime},
    u64
};
use tokio::sync::{mpsc::{Receiver, Sender}, RwLock};
use uuid::Uuid;

use super::{
    account::Account,
    config::CustodyServiceSettings,
    errors::CustodyServiceError,
    tx_parser::{self, IndexedTx, TxParser},
    types::{AccountShortInfo, Fr, RelayerState, TransactionStatusResponse}, scheduled_task::{TransferStatus, ScheduledTask},
};
use libzkbob_rs::{
    client::{TokenAmount, TxType}
};

pub enum CustodyDbColumn {
    JobsIndex,
    NullifierIndex,
}

impl Into<u32> for CustodyDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JobShortInfo {
    pub status: TransferStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JobStatusCallback {
    pub request_id: String,
    pub job_status_info: JobShortInfo,
    pub retries_left: u8,
    pub timestamp: u64,
    pub endpoint: String,
}

impl JobShortInfo {
    pub fn new() -> Self {
        Self {
            status: TransferStatus::New,
            tx_hash: None,
            failure_reason: None,
        }
    }
}

pub fn start_callback_sender(
    mut callback_receiver: Receiver<JobStatusCallback>,
    retry_queue: Sender<JobStatusCallback>,
) {
    tokio::spawn(async move {
        while let Some(job_status_callback) = callback_receiver.recv().await {
            callback_with_retries(job_status_callback, retry_queue.clone()).await;
        }
    });
}

pub async fn callback_with_retries(
    mut job_status_callback: JobStatusCallback,
    retry_queue: Sender<JobStatusCallback>,
) {
    tracing::debug!(
        "trying to deliver callback for request {}",
        &job_status_callback.request_id
    );

    let client = reqwest::Client::new();

    let body = serde_json::to_value(&job_status_callback.job_status_info).unwrap();

    let resp = client
        .post(&job_status_callback.endpoint)
        .json(&body)
        .send()
        .await;

    // if !resp.is_ok_and(|r| r.error_for_status().is_ok())  --> this is unstable
    if let Ok(r) = resp {
        if r.error_for_status().is_ok() {
            tracing::debug!(
                " request {} callback delivered",
                job_status_callback.request_id
            );
            return ();
        } else {
            tracing::debug!(
                "{} got bad status code from callback endpoint, will try again {} times",
                job_status_callback.request_id,
                job_status_callback.retries_left
            );
        }
    }
    if job_status_callback.retries_left > 0 {
        job_status_callback.retries_left -= 1;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            retry_queue.send(job_status_callback).await.unwrap();
        });
    } else {
        tracing::debug!(
            "retries exhausted for callback {} ",
            job_status_callback.request_id
        );
    }
}

pub fn start_prover(
    mut prover_receiver: Receiver<ScheduledTask>,
    status_sender: Sender<ScheduledTask>,
) {
    tokio::task::spawn(async move {
        while let Some(mut task) = prover_receiver.recv().await {
            match task.make_proof_and_send_to_relayer().await {
                Ok(_) => {
                    status_sender.send(task).await.unwrap();
                }
                Err(e) => {
                    tracing::error!("error from prover: {:#?}", &e);
                    task.update_status(TransferStatus::Failed(e)).await.unwrap();
                }
            }
        }
    });
}

pub fn start_status_updater(
    mut status_updater_receiver: Receiver<ScheduledTask>,
    status_updater_sender: Sender<ScheduledTask>,
) {
    tokio::task::spawn(async move {
        while let Some(mut task) = status_updater_receiver.recv().await {
            match task.fetch_status_with_retries().await {
                Err(CustodyServiceError::RetryNeeded) => {
                    let status_updater_sender = status_updater_sender.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(10)).await; //TODO: move to config
                        status_updater_sender.send(task).await.unwrap();
                    });
                }
                _ => (),
            };
        }
    });
}

pub struct CustodyService {
    pub settings: CustodyServiceSettings,
    pub accounts: Vec<Account>,
    pub db: Data<kvdb_rocksdb::Database>,
    next_index: Data<RwLock<u64>>
}

impl CustodyService {
    pub fn relayer_endpoint(&self, request_id: String) -> Result<String, CustodyServiceError> {
        let job_id = self
            .db
            .get(CustodyDbColumn::JobsIndex.into(), &request_id.into_bytes())
            .unwrap()
            .ok_or(CustodyServiceError::TransactionNotFound)?;
        Ok(format!(
            "{}/job/{}",
            self.settings.relayer_url,
            String::from_utf8(job_id).unwrap()
        ))
    }

    pub fn get_db(db_path: &str) -> Database {
        kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 2,
                ..Default::default()
            },
            &format!("{}/custody", db_path),
        )
        .unwrap()
    }

    pub fn validate_token(&self, bearer_token: &str) -> Result<(), CustodyServiceError> {
        if self.settings.admin_token != bearer_token {
            return Err(CustodyServiceError::AccessDenied)
        }
        Ok(())
    }

    pub fn new<D: KeyValueDB>(
        settings: CustodyServiceSettings,
        state: Data<State<D>>,
        db: Data<Database>,
    ) -> Self {
        let base_path = format!("{}/accounts_data", &settings.db_path);
        let mut accounts = vec![];

        let paths = fs::read_dir(&base_path);
        if let Ok(paths) = paths {
            tracing::info!("Loading accounts...");
            for path in paths {
                if let Ok(path) = path {
                    if path.file_type().unwrap().is_dir() {
                        let account_id = path.file_name();
                        let account_id = account_id.to_str().unwrap();
                        tracing::info!("Loading: {}", account_id);
                        let account = Account::load(&base_path, account_id).unwrap();
                        accounts.push(account);
                    }
                }
            }
        }

        let sync_interval = Duration::from_secs(settings.sync_interval_sec);
        let next_index = Data::new(RwLock::new(0));
        let next_index_clone = next_index.clone();
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            loop {
                rt.block_on(async {
                    state.sync().instrument(tracing::debug_span!("Sync custody state...")).await.unwrap();
                    let mut next_index = next_index_clone.write().await;
                    *next_index = state.finalized.lock().unwrap().next_index();
                });
                thread::sleep(sync_interval);
            }
        });

        Self {
            accounts,
            settings,
            db,
            next_index,
        }
    }

    pub fn new_account(&mut self, description: String) -> Uuid {
        let base_path = format!("{}/accounts_data", self.settings.db_path);
        let account = Account::new(&base_path, description);
        let id = account.id;
        self.accounts.push(account);
        tracing::info!("created a new account: {}", id);
        id
    }

    pub async fn gen_address(&self, account_id: Uuid) -> Option<String> {
        match self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
        {
            Some(account) => {
                let account = account.inner.read().await;
                Some(account.generate_address())
            }
            None => None,
        }
    }

    pub async fn webhook_callback_with_retries(task: ScheduledTask, queue: Sender<ScheduledTask>) {
        let client = reqwest::Client::new();

        let endpoint = task.endpoint.as_ref().unwrap();

        // let retry_task = task.clone();
        let body = serde_json::to_vec(&TransactionStatusResponse {
            status: task.status.clone(),
            tx_hash: task.tx_hash.clone(),
            failure_reason: task.failure_reason.clone(),
        })
        .unwrap();

        match client.post(endpoint).body(body).send().await {
            Ok(resp) => {
                if resp.error_for_status().is_err() {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        queue.send(task).await.unwrap();
                    })
                    .await
                    .unwrap();
                }
            }
            Err(_) => tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                queue.send(task).await.unwrap();
            })
            .await
            .unwrap(),
        }
    }

    pub async fn deposit(
        &self,
        account_id: Uuid,
        amount: u64,
        holder: String,
        params: &Parameters<Bn256>,
    ) -> Result<TransactionRequest, CustodyServiceError> {
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .unwrap();

        let account = account.inner.read().await;
        let deadline: u64 = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 1200;
        let fee: u64 = 100000000;

        let deposit_amount: Num<Fr> = Num::from_uint(NumRepr::from(amount + fee)).unwrap();
        let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();
        let holder = hex::decode(holder).unwrap();
        let deposit = TxType::DepositPermittable(
            TokenAmount::new_trimmed(fee),
            vec![],
            TokenAmount::new_trimmed(deposit_amount),
            deadline,
            holder,
        );
        let tx = account.create_tx(deposit, None, None).unwrap();

        let circuit = |public, secret| {
            c_transfer(&public, &secret, &*libzeropool::POOL_PARAMS);
        };

        let (inputs, snark_proof) = prove(params, &tx.public, &tx.secret, circuit);

        let proof = Proof {
            inputs,
            proof: snark_proof,
        };

        let tx_request = TransactionRequest {
            uuid: Some(Uuid::new_v4().to_string()),
            proof,
            memo: hex::encode(tx.memo),
            tx_type: MemoTxType::DepositPermittable.to_string(),
            deposit_signature: None,
        };

        Ok(tx_request)
    }

    pub async fn account_info(&self, account_id: Uuid) -> Option<AccountShortInfo> {
        match self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
        {
            Some(account) => Some(account.short_info().await),
            None => None,
        }
    }

    pub async fn list_accounts(&self) -> Vec<AccountShortInfo> {
        let mut res: Vec<AccountShortInfo> = vec![];

        for account in self.accounts.iter() {
            res.push(account.short_info().await);
        }

        res
    }

    pub async fn sync_account<D: KeyValueDB>(
        &self,
        account_id: Uuid,
        relayer_state: &RelayerState<D>,
    ) -> Result<(), CustodyServiceError> {
        tracing::info!("starting sync for account {}", account_id);
        let account = self.account(account_id)?;

        tracing::info!("account {} found", account_id);
        let start_index = account.next_index().await;
        let finalized_index = {
            self.next_index.read().await.clone()
        };
        tracing::info!(
            "account {}, account_index = {}, finalized index = {} ",
            account_id,
            start_index,
            finalized_index
        );

        // TODO: error instead of unwrap
        let jobs = relayer_state
            .get_jobs(start_index, finalized_index)
            .unwrap();

        let indexed_txs: Vec<IndexedTx> = jobs
            .iter()
            .map(|item| IndexedTx {
                index: item.index,
                memo: item.memo.clone(),
                commitment: item.commitment,
            })
            .collect();

        let job_indices: Vec<u64> = jobs.iter().map(|j| j.index).collect();

        tracing::info!("jobs to sync {:#?}", job_indices);

        let parse_result: ParseResult =
            TxParser::new().parse_native_tx(account.sk().await, indexed_txs);

        tracing::info!(
            "retrieved new_accounts: {:#?} \n new notes: {:#?}",
            parse_result.state_update.new_accounts,
            parse_result.state_update.new_notes
        );
        let decrypted_memos = parse_result.decrypted_memos;

        let mut batch = account.history.transaction();
        decrypted_memos.iter().for_each(|memo| {
            let jobs = relayer_state.get_jobs(memo.index, 1).unwrap();
            let job = jobs.first().unwrap();
            let tx = job.transaction.as_ref().unwrap();

            let record = HistoryRecord {
                dec_memo: memo.clone(),
                tx_hash: tx.hash,
                calldata: tx.input.0.clone(),
                block_num: tx.block_number.unwrap(),
            };

            batch.put_vec(
                HistoryDbColumn::NotesIndex.into(),
                &tx_parser::index_key(memo.index),
                serde_json::to_vec(&record).unwrap(),
            );
        });

        account.history.write(batch).unwrap();
        tracing::info!("account {} saved history", account_id);

        account.update_state(parse_result.state_update).await;
        tracing::info!("account {} state updated", account_id);

        Ok(())
    }

    pub fn account(&self, account_id: Uuid) -> Result<&Account, CustodyServiceError> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or(CustodyServiceError::AccountNotFound)
    }

    pub fn has_request_id(&self, request_id: &str) -> Result<bool, CustodyServiceError> {
        self.db.has_key(CustodyDbColumn::JobsIndex.into(), request_id.as_bytes())
            .map_err(|err| {
                tracing::error!("failed to get job id from database: {}", err);
                CustodyServiceError::DataBaseReadError
            })
    }

    pub fn get_request_id(&self, nullifier: Vec<u8>) -> Result<String, String> {
        let request_id = self
            .db
            .get(CustodyDbColumn::NullifierIndex.into(), &nullifier)
            .map_err(|err| err.to_string())?
            .ok_or("transaction id not found")?;

        let request_id = String::from_utf8(request_id).map_err(|err| err.to_string())?;

        Ok(request_id)
    }
}

impl JobStatusCallback {
    pub async fn callback_with_retries(
        self,
        endpoint: String,
        retry_queue: Sender<JobStatusCallback>,
    ) {
        let client = reqwest::Client::new();

        // let retry_task = task.clone();
        let body = serde_json::to_vec(&self.job_status_info).unwrap();

        let resp = client.post(endpoint).body(body).send().await;

        // if !resp.is_ok_and(|r| r.error_for_status().is_ok())  --> this is unstable
        if let Ok(r) = resp {
            if r.error_for_status().is_ok() {
                return ();
            }
        }
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            retry_queue.send(self).await.unwrap();
        });
    }
}