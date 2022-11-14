use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use std::{str::FromStr, sync::RwLock};
use uuid::Uuid;

use crate::{routes::job::JobResponse, state::State, types::job::Response, custody::types::TransferResponse, helpers::BytesRepr};

use super::{
    service::CustodyService,
    types::{AccountInfoRequest, SignupRequest, TransferRequest, GenerateAddressResponse, SyncResponse, SignupResponse, ListAccountsResponse, HistoryResponse, TransferStatusRequest, TransactionStatusResponse}, errors::CustodyServiceError,
};

pub type Custody = Data<RwLock<CustodyService>>;

pub async fn sync_account<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    relayer_state: Data<State<D>>,
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

    relayer_state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        CustodyServiceError::StateSyncError
    })?;

    custody.sync_account(account_id, relayer_state)?;

    Ok(HttpResponse::Ok().json(SyncResponse{
        success: true
    }))
}
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

    let state = state.finalized.lock().unwrap();
    let relayer_index = state.next_index();

    let account_info = custody.account_info(account_id, relayer_index).ok_or(
        CustodyServiceError::AccountNotFound
    )?;

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

    Ok(HttpResponse::Ok().json(SignupResponse{
        success: true,
        account_id: account_id.to_string()
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

    let finalized = state.finalized.lock().unwrap();

    Ok(HttpResponse::Ok().json(ListAccountsResponse{
        success: true,
        accounts: custody.list_accounts(finalized.next_index()
    )}))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
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
    
    custody.sync_account(account_id, state)?;

    let transaction_id = request.id.clone();
    if custody.get_job_id(&transaction_id)?.is_some() {
        return Err(CustodyServiceError::DuplicateTransactionId);
    }

    let transaction_request = vec![custody.transfer(request)?];

    let relayer_endpoint = format!("{}/sendTransactions", custody.settings.relayer_url);

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

    let response:Response = response.json().await.map_err(|e| {
        tracing::error!("the relayer response was not JSON: {:#?}", e);
        CustodyServiceError::RelayerSendError
    })?;

    // TODO: multitransfer
    let nullifier = transaction_request[0].proof.inputs[1].bytes();
    custody.save_nullifier(&transaction_id, nullifier).map_err(|err| {
        tracing::error!("failed to save nullifier: {}", err);
        CustodyServiceError::DataBaseWriteError
    })?;

    custody.save_job_id(&transaction_id, &response.job_id).map_err(|err| {
        tracing::error!("failed to save job_id: {}", err);
        CustodyServiceError::DataBaseWriteError
    })?;

    tracing::info!("relayer returned the job id: {:#?}", response.job_id );

    Ok(HttpResponse::Ok().json(TransferResponse{
        success: true,
        transaction_id
    }))
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

    let transaction_id = &request.transaction_id;
    let job_id = custody.get_job_id(transaction_id)?
        .ok_or(
            CustodyServiceError::TransactionNotFound
        )?;

    let relayer_endpoint = format!("{}/job/{}", custody.settings.relayer_url, job_id);

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
    
    Ok(HttpResponse::Ok().json(TransactionStatusResponse{
        success: true,
        state: response.state,
        tx_hash: response.tx_hash.map(|v| v[0].clone()),
        failed_reason: response.failed_reason,
    }))
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
    let address = account.generate_address();

    Ok(HttpResponse::Ok().json(GenerateAddressResponse{
        success: true,
        address,
    }))
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

    let account = custody.account(account_id)?;
    let txs = account.history(&state.pool, |nullifier: Vec<u8>| custody.get_transaction_id(nullifier)).await;

    Ok(HttpResponse::Ok().json(HistoryResponse{
        success: true,
        txs 
    }))
}
