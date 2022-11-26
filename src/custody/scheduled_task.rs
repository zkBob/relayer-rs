use std::time::{SystemTime, UNIX_EPOCH};

use actix_web::web::Data;
use kvdb::KeyValueDB;
use libzeropool::fawkes_crypto::{backend::bellman_groth16::{Parameters, engines::Bn256}, ff_uint::{Num, NumRepr}};
use libzkbob_rs::{proof::prove_tx, client::{TransactionData, TxOutput, TokenAmount, TxType}};
use serde::{Serialize, Deserialize};
use tokio::sync::{mpsc::Sender, RwLock};
use tracing_futures::Instrument;
use uuid::Uuid;
use core::fmt::Debug;

use crate::{custody::{routes::fetch_tx_status, service::JobStatusCallback, types::JobShortInfo}, types::{transaction_request::{TransactionRequest, Proof}, job::Response}, helpers::BytesRepr, state::State};

use super::{errors::CustodyServiceError, service::{CustodyDbColumn, CustodyService}, types::Fr};
use memo_parser::memo::TxType as MemoTxType;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    New,
    Queued,
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

impl TransferStatus {
    pub fn is_final(&self) -> bool {
        match self {
            TransferStatus::Done | TransferStatus::Failed(_) => true,
            _ => false
        }
    }
}

pub struct ScheduledTask<D:'static + KeyValueDB> {
    pub request_id: String,
    pub task_index: u32,
    pub account_id: Uuid,
    pub db: Data< kvdb_rocksdb::Database>,
    pub job_id: Option<Vec<u8>>,
    pub endpoint: Option<String>,
    pub relayer_url: String,
    pub retries_left: u8,
    pub status: TransferStatus,
    pub tx_hash: Option<String>,
    pub failure_reason: Option<String>,
    pub callback_address: Option<String>,
    pub params: Data<Parameters<Bn256>>,
    pub tx: Option<TransactionData<Fr>>,
    pub callback_sender: Data<Sender<JobStatusCallback>>,
    
    pub amount: Num<Fr>,
    pub to: Option<String>,
    pub custody: Data<RwLock<CustodyService>>,
    pub state: Data<State<D>>,
    pub depends_on: Option<Vec<u8>>
}

impl<D: KeyValueDB> Debug for ScheduledTask<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledTask")
            .field("task_index", &self.task_index)
            .field("request_id", &self.request_id)
            .field("job_id", &self.job_id)
            .field("endpoint", &self.endpoint)
            .field("relayer_url", &self.relayer_url)
            .field("retries_left", &self.retries_left)
            .field("status", &self.status)
            .field("tx_hash", &self.tx_hash)
            .field("failure_reason", &self.failure_reason)
            
            .finish()
    }
}

impl<D: KeyValueDB> ScheduledTask<D> {
    pub async fn make_proof_and_send_to_relayer(&mut self) -> Result<(), CustodyServiceError> {
        let request_id = self.request_id.clone();
        {
            let tx = self.tx.clone().unwrap();

            let proving_span = tracing::info_span!("proving", request_id = request_id.clone());
        
            let (inputs, proof) = proving_span.in_scope(|| {
                prove_tx(
                    &self.params,
                    &*libzeropool::POOL_PARAMS,
                    tx.public,
                    tx.secret,
                )
            });

            let proof = Proof { inputs, proof };

            let tx_request = vec![TransactionRequest {
                uuid: Some(Uuid::new_v4().to_string()),
                proof,
                memo: hex::encode(tx.memo),
                tx_type: format!("{:0>4}", MemoTxType::Transfer.to_u32()),
                deposit_signature: None,
            }];

            let relayer_endpoint = format!("{}/sendTransactions", self.relayer_url);

            let response = reqwest::Client::new()
                .post(relayer_endpoint)
                .json(&tx_request)
                .header("Content-type", "application/json")
                .send()
                .instrument(tracing::info_span!(
                    "sending to relayer",
                    request_id = request_id.clone()
                ))
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
            let nullifier = tx_request[0].proof.inputs[1].bytes();
            self.save_nullifier(nullifier).map_err(|err| {
                tracing::error!("failed to save nullifier: {}", err);
                CustodyServiceError::DataBaseWriteError(err.to_string())
            })?;

            tracing::info!("relayer returned the job id: {:#?}", response.job_id);

            let job_id = response.job_id.clone();

            self.endpoint = Some(format!("{}/job/{}", self.relayer_url, job_id.clone()));

            self.job_id = Some(job_id.into_bytes());
        }

        self.update_status(TransferStatus::Relaying)
            .instrument(tracing::info_span!(
                "updating status, new status: Relaying",
                request_id = request_id
            ))
            .await
            .map_err(|err| {
                tracing::error!("failed to save job_id: {}", err);
                CustodyServiceError::DataBaseWriteError(err.to_string())
            })?;

        Ok(())
    }

    pub async fn fetch_status_with_retries(&mut self) -> Result<(), CustodyServiceError> {
        tracing::info!(
            "fetchin status for request_id {}, task index {}, retries left {}",
            &self.request_id,
            &self.task_index,
            &self.retries_left
        );
        
        let endpoint = self.endpoint.as_ref().unwrap();
        match fetch_tx_status(&endpoint).await {
            //we have successfuly retrieved job status from relayer
            Ok(relayer_response) => {
                match relayer_response.tx_hash {
                    // transaction has been mined, set tx_hash and status
                    //TODO: call webhook on_complete
                    Some(tx_hash) => {
                        let status = TransferStatus::from(relayer_response.status);
                        if status != self.status || self.tx_hash != Some(tx_hash.clone()) {
                            self.tx_hash = Some(tx_hash);
                            self.update_status(status.clone()).await.unwrap();
                        }

                        match status {
                            TransferStatus::Done => {
                                tracing::info!("marked task {} of request with id {} as done", &self.task_index, &self.request_id);
                                Ok(())
                            }
                            TransferStatus::Failed(_) => {
                                Ok(())
                            }
                            _ => {
                                Err(CustodyServiceError::RetryNeeded)   
                            }
                        }
                        
                    }
                    // transaction not mined
                    None => {
                        // transaction rejected
                        if let Some(failure_reason) = relayer_response.failure_reason {
                            self.status = TransferStatus::Failed(
                                CustodyServiceError::TaskRejectedByRelayer(failure_reason.clone()),
                            );
                            self.update_status(TransferStatus::Failed(
                                CustodyServiceError::TaskRejectedByRelayer(failure_reason),
                            ))
                            .await
                            .unwrap();
                            Ok(())
                        // waiting for transaction to be mined, schedule a retry
                        } else {
                            Err(CustodyServiceError::RetryNeeded)
                        }
                    }
                }
            }
            //we couldn't get valid response
            Err(_) => {
                // let mut task = self.clone();
                if self.retries_left > 0 {
                    self.retries_left -= 1;
                    Err(CustodyServiceError::RetryNeeded)
                } else {
                    //if no more retries left, mark request as failed to prevent resource exhaustion
                    //TODO: increase lag exponentialy
                    tracing::error!("retries exhausted for task {:#?}", self);
                    self.update_status(TransferStatus::Failed(
                        CustodyServiceError::RetriesExhausted,
                    ))
                    .await
                    .unwrap();
                    Err(CustodyServiceError::RetriesExhausted)
                }
            }
        }
    }

    pub fn save_nullifier(&self, nullifier: Vec<u8>) -> Result<(), String> {
        let nullifier_exists = self.db.has_key(
            CustodyDbColumn::NullifierIndex.into(), 
            &nullifier
        ).unwrap();

        if !nullifier_exists {
            let tx = {
                let mut tx = self.db.transaction();
                tx.put(
                    CustodyDbColumn::NullifierIndex.into(),
                    &nullifier,
                    self.request_id.as_bytes(),
                );
                tx
            };
            self.db.write(tx).map_err(|err| err.to_string())
        } else {
            // TODO: what to do in this case?
            // we shouldn't overwrite existed value
            Ok(())
        }
    }

    pub async fn update_status(
        &mut self,
        status: TransferStatus,
    ) -> Result<(), CustodyServiceError> {
        tracing::info!("update status initiated");
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let job_status_info = JobShortInfo {
            status: status.clone(),
            tx_hash: self.tx_hash.clone(),
            failure_reason: self.failure_reason.clone(),
            amount: self.amount.to_string(),
            to: self.to.clone(),
            timestamp,
        };

        let tx = {
            let mut tx = self.db.transaction();
            tx.put(
                CustodyDbColumn::JobsIndex.into(),
                &self.task_key(),
                &serde_json::to_vec(&job_status_info).unwrap(),
            );
            tx
        };
        self.db
            .write(tx)
            .map_err(|err| CustodyServiceError::DataBaseWriteError(err.to_string()))?;

        if let Some(endpoint) = self.callback_address.clone() {
            tracing::info!("trying to deliver callback");
            let job_status_callback = JobStatusCallback {
                request_id: self.request_id.clone(),
                endpoint,
                job_status_info,
                retries_left: 42,
                timestamp,
            };

            self.callback_sender
                .send(job_status_callback)
                .await
                .unwrap();
        }

        Ok(())
    }

    pub async fn prepare_task(&mut self) -> Result<TransferStatus, CustodyServiceError> {
        let previous_status = {
            if self.depends_on.is_none() {
                // first task is always ready
                TransferStatus::Done
            } else {
                let previous_job = self.db.get(
                    CustodyDbColumn::JobsIndex.into(), 
                    &self.depends_on.clone().unwrap()
                )
                .map_err(|_| CustodyServiceError::DataBaseReadError)?.unwrap();
    
                let previous_job: JobShortInfo = serde_json::from_slice(&previous_job).map_err(|_| CustodyServiceError::DataBaseReadError)?;
                previous_job.status
            }
        };

        match previous_status {
            TransferStatus::Done => {
                // TODO: replace this with optimistic state
                match self.state.sync().await {
                    Ok(_) => { 
                        let tx = {
                            let custody = self.custody.read().await;
                            custody.sync_account(self.account_id, &self.state).await?;
                            let account = custody.account(self.account_id)?;
                            let account = account.inner.read().await;
        
                            // TODO: move to config
                            let fee: u64 = 100000000;
                            let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();
        
                            let tx_outputs = match &self.to {
                                Some(to) => {
                                    vec![TxOutput {
                                        to: to.clone(),
                                        amount: TokenAmount::new(self.amount),
                                    }]
                                },
                                None => vec![],
                            };
                            let transfer = TxType::Transfer(TokenAmount::new(fee), vec![], tx_outputs);
        
                            account.create_tx(transfer, None, None)
                                .map_err(|e| CustodyServiceError::BadRequest(e.to_string()))
                        };
        
                        match tx {
                            Ok(tx) => {
                                self.tx = Some(tx);
                                self.update_status(TransferStatus::Proving).await?;
                                Ok(TransferStatus::Proving)
                            },
                            Err(err) => {
                                self.update_status(TransferStatus::Failed(err.clone())).await?;
                                Ok(TransferStatus::Failed(err))
                            }
                        }
                     }
                    Err(err) => {
                        tracing::warn!("failed to sync state with error: {:?}", err);
                        Ok(TransferStatus::Queued)
                    }
                }
            }
            TransferStatus::Failed(_) => {
                self.update_status(TransferStatus::Failed(CustodyServiceError::PreviousTxFailed)).await?;
                Ok(TransferStatus::Failed(CustodyServiceError::PreviousTxFailed))
            }
            _ => {
                Ok(TransferStatus::Queued)
            }
        }
    }

    pub fn task_key(&self) -> Vec<u8> {
        [self.request_id.as_bytes(), &self.task_index.to_be_bytes()].concat()
    }
}
