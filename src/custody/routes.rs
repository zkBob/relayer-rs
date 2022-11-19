use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use kvdb_rocksdb::Database;
use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters};
use std::{str::FromStr, sync::RwLock};
use uuid::Uuid;

use crate::{custody::types::TransferResponse, routes::job::JobResponse, state::State};

use super::{
    errors::CustodyServiceError,
    service::{CustodyService, TransferStatus},
    types::{
        AccountInfoRequest, GenerateAddressResponse, ScheduledTask, SignupRequest, SignupResponse,
        TransactionStatusResponse, TransferRequest, TransferStatusRequest,
    },
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

    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        CustodyServiceError::StateSyncError
    })?;

    custody.sync_account(account_id, &state).await?;

    let account_info = custody
        .account_info(account_id)
        .await
        .ok_or(CustodyServiceError::AccountNotFound)?;

    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    _state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let mut custody = custody.write().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    let account_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn list_accounts<D: KeyValueDB>(
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        CustodyServiceError::StateSyncError
    })?;

    Ok(HttpResponse::Ok().json(custody.list_accounts().await))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
    params: Data<Parameters<Bn256>>,
    custody_db: Data<Database>,
) -> Result<HttpResponse, CustodyServiceError> {
    let request: TransferRequest = request.0.into();

    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    let relayer_url = custody.settings.relayer_url.clone();
    let account = custody.account(account_id).unwrap();

    // custody.sync_account(account_id, &state)?;

        // account.sync(&state).await?;
    let request_id = request
        .request_id
        .clone()
        .unwrap_or(Uuid::new_v4().to_string());
    // if custody.get_job_id_by_request_id(&request_id)?.is_some() {
    //     return Err(CustodyServiceError::DuplicateTransactionId);
    // }

    // request.request_id = Some(request_id.clone());

    let task = ScheduledTask {
        request_id: request_id.clone(),
        request,
        job_id: None,
        endpoint: None,
        retries_left: 42,
        status: TransferStatus::Proving,
        tx_hash: None,
        failure_reason: None,
        account:Data::new(account),
        relayer_url,
        params,
        db: custody_db,
    };

    // custody.update_task_status(task.clone(), TransferStatus::New).await?;
    // task.update_status(TransferStatus::Proving).await.unwrap();

    custody.sender.send(task).await.unwrap();

    Ok(HttpResponse::Ok().json(TransferResponse {
        request_id: request_id.clone(),
    }))
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

    let response: JobResponse = response.json().await.map_err(|e| {
        tracing::error!("the relayer response was not JSON: {:#?}", e);
        CustodyServiceError::RelayerSendError
    })?;

    Ok(TransactionStatusResponse {
        status: TransferStatus::from(response.state),
        tx_hash: response.tx_hash,
        failure_reason: response.failed_reason,
    })
}
pub async fn transaction_status<D: KeyValueDB>(
    request: Query<TransferStatusRequest>,
    _: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    let relayer_endpoint = custody.relayer_endpoint(request.0.request_id)?;
    let transaction_status = fetch_tx_status(&relayer_endpoint).await?;

    Ok(HttpResponse::Ok().json(transaction_status))
}

pub async fn generate_shielded_address<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

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

    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    custody.sync_account(account_id, &state).await?;

    let account = custody.account(account_id)?;
    let txs = account
        .history(&state.pool, |nullifier: Vec<u8>| {
            custody.get_request_id(nullifier)
        })
        .await;

    Ok(HttpResponse::Ok().json(txs))
}
