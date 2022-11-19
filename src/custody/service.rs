use crate::{
    custody::{
        routes::fetch_tx_status,
        tx_parser::ParseResult,
        types::{HistoryDbColumn, HistoryRecord},
    },
    helpers::BytesRepr,
    state::State,
    types::{
        job::Response,
        transaction_request::{Proof, TransactionRequest},
    },
};
use actix_web::web::Data;
use kvdb::KeyValueDB;
use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::ff_uint::NumRepr;
use libzeropool::{
    circuit::tx::c_transfer,
    fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters},
    fawkes_crypto::{backend::bellman_groth16::prover::prove, ff_uint::Num},
};
use memo_parser::memo::TxType as MemoTxType;
use serde::{Deserialize, Serialize};
use std::{
    fs, thread,
    time::{Duration, SystemTime},
};
use tokio::sync::{mpsc::{self, Receiver, Sender}, RwLock};
use uuid::Uuid;

use super::{
    account::Account,
    config::CustodyServiceSettings,
    errors::CustodyServiceError,
    tx_parser::{self, IndexedTx, TxParser},
    types::{
        AccountShortInfo, Fr, RelayerState, ScheduledTask,
        TransactionStatusResponse, TransferRequest,
    },
};
use libzkbob_rs::{
    client::{TokenAmount, TxOutput, TxType},
    proof::prove_tx,
};
use std::str::FromStr;

pub enum CustodyDbColumn {
    JobsIndex,
    NullifierIndex,
}

impl Into<u32> for CustodyDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}

pub struct CustodyService {
    pub settings: CustodyServiceSettings,
    pub accounts: Vec<Account>,
    pub params: Parameters<Bn256>,
    pub db: kvdb_rocksdb::Database,
    pub sender: Sender<ScheduledTask>,
    //pub receiver: Receiver<ScheduledTask>,
    pub status_sender: Sender<ScheduledTask>,
    //pub status_receiver: Receiver<ScheduledTask>,
    pub webhook_sender: Sender<ScheduledTask>,
    pub webhook_receiver: Receiver<ScheduledTask>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TransferStatus {
    New,
    Proving,
    Relaying,
    Mining,
    Done,
    Failed(CustodyServiceError),
}

impl From<String> for TransferStatus {
    fn from(val: String) -> Self {
        match val.as_str() {
            "waiting" => Self::Relaying,
            "sent" => Self::Mining,
            "reverted" => Self::Failed(CustodyServiceError::RelayerSendError), // TODO: fix error
            "completed" => Self::Done,
            "failed" => Self::Failed(CustodyServiceError::RelayerSendError),
            _ => Self::Failed(CustodyServiceError::RelayerSendError),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct JobShortInfo {
    pub job_id: Option<String>,
    pub status: TransferStatus,
    pub tx_hash: Option<String>,
    pub failure_reason: Option<String>,
}

impl CustodyService {
    pub fn start_relayer_sender(custody: Data<RwLock<CustodyService>>, mut receiver: Receiver<ScheduledTask>) {
        tokio::task::spawn(async move {
            tracing::info!("starting relayer sender...");
            
            while let Some(task) = receiver.recv().await {
                let scheduled_task = task.clone();
                let custody = custody.read().await;

                match custody.send_tx_to_relayer(scheduled_task).await {
                    Ok(_) => tracing::debug!("sent to status fetch queue {:#?}", task),
                    Err(_) => tracing::error!("failed to process {:#?}", task),
                };
            }
        });
    }

    pub fn start_status_processor(custody: Data<RwLock<CustodyService>>, mut receiver: Receiver<ScheduledTask>) {
        tokio::task::spawn(async move {
            tracing::info!("starting status processor...");

            while let Some(task) = receiver.recv().await {
                let custody = custody.read().await;
                let sender = custody.status_sender.clone();

                custody.fetch_status_with_retries(task, sender).await;
            }
        });
    }

    pub fn relayer_endpoint(&self, job_id: &str) -> Result<String, CustodyServiceError> {
        Ok(format!(
            "{}/job/{}",
            self.settings.relayer_url,
            job_id
        ))
    }

    pub async fn send_tx_to_relayer(
        &self,
        mut task: ScheduledTask,
    ) -> Result<(), CustodyServiceError> {
        let transaction_request = vec![self.transfer(task.request.clone())?];
        let relayer_endpoint = format!("{}/sendTransactions", self.settings.relayer_url);

        let response = reqwest::Client::new()
            .post(relayer_endpoint)
            .json(&transaction_request)
            .header("Content-type", "application/json")
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    "network exception when sending request to relayer: {:#?}",
                    e
                );
                CustodyServiceError::RelayerSendError
            })?;

        let response = response.error_for_status().map_err(|e| {
            tracing::error!("relayer returned bad status code {:#?}", e);
            CustodyServiceError::RelayerSendError
        })?;

        let response: Response = response.json().await.map_err(|e| {
            tracing::error!("the relayer response was not JSON: {:#?}", e);
            CustodyServiceError::RelayerSendError
        })?;

        // TODO: multitransfer
        let nullifier = transaction_request[0].proof.inputs[1].bytes();
        self.save_nullifier(&task.request_id, nullifier)
            .map_err(|err| {
                tracing::error!("failed to save nullifier: {}", err);
                CustodyServiceError::DataBaseWriteError(err.to_string())
            })?;

        tracing::info!("relayer returned the job id: {:#?}", response.job_id);

        let job_id = response.job_id.clone();

        task.job_id = Some(job_id);
        task.status = TransferStatus::Relaying;
        self.update_task_status(task.clone(), TransferStatus::Relaying)
            .await
            .map_err(|err| {
                tracing::error!("failed to save job_id: {}", err);
                CustodyServiceError::DataBaseWriteError(err.to_string())
            })?;

        self.status_sender
            .send(task)
            .await
            .map_err(|e| CustodyServiceError::InternalError(e.to_string()))?;
        Ok(())
    }
    
    pub fn new<D: KeyValueDB>(
        params: Parameters<Bn256>,
        settings: CustodyServiceSettings,
        state: Data<State<D>>,
    ) -> Data<RwLock<Self>> {
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

        let db = kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 2,
                ..Default::default()
            },
            &format!("{}/custody", &settings.db_path),
        )
        .unwrap();

        let sync_interval = Duration::from_secs(settings.sync_interval_sec);
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            loop {
                tracing::debug!("Sync custody state...");
                rt.block_on(state.sync()).unwrap();
                thread::sleep(sync_interval);
            }
        });

        let (sender, receiver) = mpsc::channel::<ScheduledTask>(100);
        let (status_sender, status_receiver) = mpsc::channel::<ScheduledTask>(100);
        let (webhook_sender, webhook_receiver) = mpsc::channel::<ScheduledTask>(100);

        let custody = Self {
            accounts,
            settings,
            params,
            db,
            sender,
            //receiver,
            status_sender,
            //status_receiver,
            webhook_sender,
            webhook_receiver,
        };

        let c = Data::new(RwLock::new(custody));
        CustodyService::start_relayer_sender(c.clone(), receiver);
        CustodyService::start_status_processor(c.clone(), status_receiver);
        c
    }

    pub fn new_account(&mut self, description: String) -> Uuid {
        let base_path = format!("{}/accounts_data", self.settings.db_path);
        let account = Account::new(&base_path, description);
        let id = account.id;
        self.accounts.push(account);
        tracing::info!("created a new account: {}", id);
        id
    }

    pub fn gen_address(&self, account_id: Uuid) -> Option<String> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| {
                let account = account.inner.read().unwrap();
                account.generate_address()
            })
    }

    pub async fn webhook_callback_with_retries(task: ScheduledTask, queue: Sender<ScheduledTask>) {
        let client = reqwest::Client::new();

        let endpoint = task.endpoint.as_ref().unwrap();

        let retry_task = task.clone();
        let body = serde_json::to_vec(&TransactionStatusResponse {
            status: task.status,
            tx_hash: task.tx_hash,
            failure_reason: task.failure_reason,
        })
        .unwrap();

        match client
            .post(endpoint)
            .body(body)
            .send()
            .await {
                Ok(resp) => {
                    if resp.error_for_status().is_err() {
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                            queue.send(retry_task).await.unwrap();
                        }).await.unwrap();
                    }
                },
                Err(_) => tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    queue.send(retry_task).await.unwrap();
                }).await.unwrap(),
            }
            
    }

    pub async fn fetch_status_with_retries(
        &self,
        mut task: ScheduledTask,
        queue: Sender<ScheduledTask>,
    ) -> () {
        let endpoint = self.relayer_endpoint(&task.job_id.clone().unwrap()).unwrap();
        match fetch_tx_status(&endpoint).await {
            //we have successfuly retrieved job status from relayer
            Ok(relayer_response) => {
                match relayer_response.tx_hash {
                    // transaction has been mined, set tx_hash and status
                    //TODO: call webhook on_complete
                    Some(tx_hash) => {
                        // let mut task = task.clone();
                        task.tx_hash = Some(tx_hash);
                        task.status = TransferStatus::Done;
                        self.update_task_status(task, TransferStatus::Done)
                            .await
                            .unwrap();
                    }
                    // transaction not mined
                    None => {
                        // transaction rejected
                        if let Some(failure_reason) = relayer_response.failure_reason {
                            task.status = TransferStatus::Failed(
                                CustodyServiceError::TaskRejectedByRelayer(failure_reason.clone()),
                            );
                            self.update_task_status(
                                task,
                                TransferStatus::Failed(CustodyServiceError::TaskRejectedByRelayer(
                                    failure_reason,
                                )),
                            )
                            .await
                            .unwrap();
                        // waiting for transaction to be mined, schedule a retry
                        } else {
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_secs(30)).await;
                                queue.send(task).await.unwrap();
                            });
                        }
                    }
                };
            }
            //we couldn't get valid response
            Err(_) => {
                let mut task = task.clone();
                if task.retries_left > 0 {
                    task.retries_left -= 1;
                    //schedule a retry, reduce retry count
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        queue.send(task).await.unwrap();
                    });
                } else {
                    //if no more retries left, mark request as failed to prevent resource exhaustion
                    //TODO: increase lag exponentialy
                    task.status = TransferStatus::Failed(CustodyServiceError::RetriesExhausted);
                    tracing::error!("retries exhausted for task {:#?}", task);
                    self.update_task_status(
                        task,
                        TransferStatus::Failed(CustodyServiceError::RetriesExhausted),
                    )
                    .await
                    .unwrap();
                }
            }
        }
    }

    pub fn transfer(
        &self,
        request: TransferRequest,
    ) -> Result<TransactionRequest, CustodyServiceError> {
        let account_id = Uuid::from_str(&request.account_id).unwrap();
        let account = self.account(account_id)?;
        let account = account.inner.read().unwrap();

        let fee = 100000000;
        let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();

        let tx_output: TxOutput<Fr> = TxOutput {
            to: request.to,
            amount: TokenAmount::new(Num::from_uint(NumRepr::from(request.amount)).unwrap()),
        };
        let transfer = TxType::Transfer(TokenAmount::new(fee), vec![], vec![tx_output]);

        let tx = account.create_tx(transfer, None, None).unwrap();
        let (inputs, proof) = prove_tx(
            &self.params,
            &*libzeropool::POOL_PARAMS,
            tx.public,
            tx.secret,
        );

        let proof = Proof { inputs, proof };

        let tx_request = TransactionRequest {
            uuid: Some(Uuid::new_v4().to_string()),
            proof,
            memo: hex::encode(tx.memo),
            tx_type: format!("{:0>4}", MemoTxType::Transfer.to_u32()),
            deposit_signature: None,
        };

        Ok(tx_request)
    }

    pub fn deposit(
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

        let account = account.inner.read().unwrap();
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

    pub fn account_info(&self, account_id: Uuid) -> Option<AccountShortInfo> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(Account::short_info)
    }

    pub fn list_accounts(&self) -> Vec<AccountShortInfo> {
        self.accounts.iter().map(Account::short_info).collect()
    }

    pub fn sync_account<D: KeyValueDB>(
        &self,
        account_id: Uuid,
        relayer_state: &RelayerState<D>,
    ) -> Result<(), CustodyServiceError> {
        tracing::info!("starting sync for account {}", account_id);
        let account = self.account(account_id)?;

        tracing::info!("account {} found", account_id);
        let start_index = account.next_index();
        let finalized_index = {
            let finalized = relayer_state.finalized.lock().unwrap();
            finalized.next_index()
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

        let parse_result: ParseResult = TxParser::new().parse_native_tx(account.sk(), indexed_txs);

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

        account.update_state(parse_result.state_update);
        tracing::info!("account {} state updated", account_id);

        Ok(())
    }

    pub fn account(&self, account_id: Uuid) -> Result<&Account, CustodyServiceError> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or(CustodyServiceError::AccountNotFound)
    }

    pub async fn update_task_status(
        &self,
        task: ScheduledTask,
        // request_id: &str,
        // job_id: Option<Vec<u8>>,
        status: TransferStatus,
        // tx_hash: Option<String>,
    ) -> Result<(), CustodyServiceError> {
        let webhook_task = task.clone();
        let tx = {
            let mut tx = self.db.transaction();
            tx.put(
                CustodyDbColumn::JobsIndex.into(),
                task.request_id.as_bytes(),
                &serde_json::to_vec(&JobShortInfo {
                    job_id: task.job_id,
                    status: status.clone(),
                    tx_hash: task.tx_hash,
                    failure_reason: task.failure_reason,
                })
                .unwrap(),
            );
            tx
        };
        self.db
            .write(tx)
            .map_err(|err| CustodyServiceError::DataBaseWriteError(err.to_string()))?;

        if webhook_task.endpoint.is_some() {
            match status {
                TransferStatus::Done | TransferStatus::Failed(_) => {
                    self.webhook_sender.send(webhook_task).await.unwrap()
                }
                _ => (),
            }
        }

        Ok(())
    }

    pub fn get_job_info_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Option<JobShortInfo>, CustodyServiceError> {
        self.db
            .get(CustodyDbColumn::JobsIndex.into(), request_id.as_bytes())
            .map_err(|err| {
                tracing::error!("failed to get job id from database: {}", err);
                CustodyServiceError::DataBaseReadError
            })?
            .map(|value| {
                let job_short_info: JobShortInfo = serde_json::from_slice(&value).unwrap();
                Ok(job_short_info)
            })
            .map_or(Ok(None), |v| v.map(Some))
    }

    pub fn has_request_id(&self, request_id: &str) -> Result<bool, CustodyServiceError> {
        self.db.has_key(CustodyDbColumn::JobsIndex.into(), request_id.as_bytes())
            .map_err(|err| {
                tracing::error!("failed to get job id from database: {}", err);
                CustodyServiceError::DataBaseReadError
            })
    }

    pub fn save_nullifier(&self, request_id: &str, nullifier: Vec<u8>) -> Result<(), String> {
        let tx = {
            let mut tx = self.db.transaction();
            tx.put(
                CustodyDbColumn::NullifierIndex.into(),
                &nullifier,
                request_id.as_bytes(),
            );
            tx
        };
        self.db.write(tx).map_err(|err| err.to_string())
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
