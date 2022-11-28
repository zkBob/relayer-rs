use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use kvdb::KeyValueDB;
use kvdb_rocksdb::Database;
use libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{engines::Bn256, Parameters},
};

use std::str::FromStr;
use tokio::sync::{mpsc::Sender, RwLock};
use uuid::Uuid;

use crate::{
    custody::{types::TransferResponse, scheduled_task::TransferStatus},
    routes::job::JobResponse,
    state::State,
};

use super::{
    errors::CustodyServiceError,
    service::{CustodyService, JobStatusCallback, CustodyDbColumn},
    types::{
        AccountInfoRequest, CalculateFeeResponse, GenerateAddressResponse, JobShortInfo,
        SignupRequest, SignupResponse, TransactionStatusResponse, TransferRequest,
        TransferStatusRequest, CustodyTransactionStatusResponse, CustodyHistoryRecord, CalculateFeeRequest,
    }, scheduled_task::ScheduledTask,
};

pub type Custody = Data<RwLock<CustodyService>>;

pub async fn account_info<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    custody.sync_account(account_id, &state).await?;

    let account_info = custody
        .account_info(account_id, state.settings.web3.relayer_fee)
        .await
        .ok_or(CustodyServiceError::AccountNotFound)?;

    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn calculate_fee<D: KeyValueDB>(
    request: Json<CalculateFeeRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let request: CalculateFeeRequest = request.0.into();

    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;    
    custody.sync_account(account_id, &state).await?;

    let account = custody.account(account_id)?;

    let fee = state.settings.web3.relayer_fee;

    let tx_parts = account
        .get_tx_parts(request.amount, fee, String::default())
        .await?;

    let transaction_count: u64 = tx_parts.len() as u64;

    let total_fee = fee * transaction_count;

    Ok(HttpResponse::Ok()
        .json(&CalculateFeeResponse {
            transaction_count,
            total_fee,
        })
        )
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    custody: Data<RwLock<CustodyService>>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CustodyServiceError> {
    let mut custody = custody.write().await;
    custody.validate_token(bearer.token())?;

    let account_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn list_accounts<D: KeyValueDB>(
    custody: Custody,
    bearer: BearerAuth,
    state: Data<State<D>>
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;
    custody.validate_token(bearer.token())?;

    Ok(HttpResponse::Ok().json(custody.list_accounts(state).await?))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
    params: Data<Parameters<Bn256>>,
    custody_db: Data<Database>,
    prover_sender: Data<Sender<ScheduledTask<D>>>,
    callback_sender: Data<Sender<JobStatusCallback>>,
) -> Result<HttpResponse, CustodyServiceError> {
    let request: TransferRequest = request.0.into();

    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody_clone = custody.clone();
    let custody = custody.write().await;
    let relayer_url = custody.settings.relayer_url.clone();
    custody.sync_account(account_id, &state).await?;

    let request_id = request
        .request_id
        .clone()
        .unwrap_or(Uuid::new_v4().to_string());
    if custody.has_request_id(&request_id)? {
        return Err(CustodyServiceError::DuplicateTransactionId);
    }

    let account = custody.account(account_id)?;
    let last_account_task = account.last_task().await;
    if let Some(task_key) = last_account_task {
        let job = custody
            .db
            .get(
                CustodyDbColumn::JobsIndex.into(),
                &task_key,
            )
            .map_err(|_| CustodyServiceError::DataBaseReadError)?
            .ok_or(CustodyServiceError::TransactionNotFound)?;
        
        let job: JobShortInfo = serde_json::from_slice(&job).map_err(|_| CustodyServiceError::DataBaseReadError)?;
        match job.status {
            TransferStatus::Done => {
                let txs = account
                    .history(|nullifier: Vec<u8>| {
                        custody.get_request_id(nullifier)
                    }, None)
                    .await;
                let synced_tx = txs.iter().find(|tx| tx.tx_hash == job.tx_hash.clone().unwrap());
                if synced_tx.is_none() {
                    return Err(CustodyServiceError::AccountIsNotSynced);
                }
            }
            TransferStatus::Failed(_) => (),
            _ => {
                return Err(CustodyServiceError::AccountIsBusy);
            }
        }
    }

    let fee: u64 = state.settings.web3.relayer_fee;

    let tx_parts = account.get_tx_parts(request.amount, fee, request.to.clone()).await?;
    if prover_sender.capacity() - 1 < tx_parts.len() {
        return Err(CustodyServiceError::ServiceIsBusy);
    }

    let mut depends_on = None;
    let tx_seq_len = tx_parts.len();
    for (i, (to, amount)) in tx_parts.iter().enumerate() {

        let last_in_seq = i + 1 == tx_seq_len;
        let mut task = ScheduledTask {
            request_id: request_id.clone(),
            task_index: i as u32,
            account_id,
            job_id: None,
            endpoint: None,
            retries_left: 42,
            status: TransferStatus::New,
            tx_hash: None,
            relayer_url: relayer_url.clone(),
            params: params.clone(),
            db: custody_db.clone(),
            tx: None,
            callback_address: request.webhook.clone(),
            callback_sender: callback_sender.clone(),
            amount: amount.clone(),
            fee,
            to: to.clone(),
            custody: custody_clone.clone(),
            state: state.clone(),
            depends_on,
            last_in_seq
        };

        let status = if i == 0 { TransferStatus::New } else { TransferStatus::Queued };
        task.update_status(status).await?;
        
        let task_key = task.task_key();
        depends_on = Some(task_key.clone());
        account.update_last_task(task_key).await;
        
        prover_sender.send(task).await.unwrap();
    }

    custody.save_tasks_count(&request_id, tx_seq_len as u32)?;

    tracing::info!(
        "{} request received & saved, tx created and sent to the prover queue",
        &request_id
    );

    Ok(HttpResponse::Ok().json(TransferResponse {
        request_id: request_id.clone(),
    }))
}


pub async fn callback_mock(
    callback: Json<JobShortInfo> ) -> Result <HttpResponse, CustodyServiceError>
{
    let callback: JobShortInfo = callback.0.into();

    tracing::info!("received callback for request id {:}, new status {:?}",
     callback.request_id, 
    callback.status);

    Ok(HttpResponse:: Ok().finish())
}
pub async fn fetch_tx_status(
    relayer_endpoint: &str,
) -> Result<TransactionStatusResponse, CustodyServiceError> {
    let response = reqwest::Client::new()
        .get(relayer_endpoint)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "network exception when sending request to relayer: {:#?}",
                e
            );
            CustodyServiceError::RelayerSendError
        })?;

    let body = response.text().await.unwrap();

    match serde_json::from_str::<JobResponse>(&body) {
        Ok(response) => Ok(TransactionStatusResponse {
            status: response.state,
            tx_hash: response.tx_hash,
            failure_reason: response.failed_reason,
        }),
        Err(e) => {
            tracing::error!("the relayer response {:#?} was not JSON: {:#?}", body, e);
            Err(CustodyServiceError::RelayerSendError)
        }
    }
}

pub async fn transaction_status<D: KeyValueDB>(
    request: Query<TransferStatusRequest>,
    _: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;

    let task_keys = custody.task_keys(&request.0.request_id)?;
    let mut jobs = vec![];
    for task_key in task_keys {
        let job = custody
            .db
            .get(
                CustodyDbColumn::JobsIndex.into(),
                &task_key,
            )
            .map_err(|_| CustodyServiceError::DataBaseReadError)?
            .ok_or(CustodyServiceError::TransactionNotFound)?;

        let job: JobShortInfo =
            serde_json::from_slice(&job).map_err(|_| CustodyServiceError::DataBaseReadError)?;
        jobs.push(job);
    }

    Ok(HttpResponse::Ok().json(CustodyTransactionStatusResponse::from(jobs)))
}

pub async fn generate_shielded_address<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    let account = custody.account(account_id)?;
    let address = account.generate_address().await;

    Ok(HttpResponse::Ok().json(GenerateAddressResponse { address }))
}

pub async fn history<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    custody.sync_account(account_id, &state).await?;

    let account = custody.account(account_id)?;
    let txs = account
        .history(|nullifier: Vec<u8>| {
            custody.get_request_id(nullifier)
        }, Some(&state.pool))
        .await;

    Ok(HttpResponse::Ok().json(CustodyHistoryRecord::convert_vec(txs)))
}
