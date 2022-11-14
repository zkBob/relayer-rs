use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use std::{str::FromStr, sync::RwLock};
use uuid::Uuid;

use crate::{routes::{ServiceError, job::JobResponse}, state::State, types::job::Response, custody::types::TransferResponse, helpers::BytesRepr};

use super::{
    service::CustodyService,
    types::{AccountInfoRequest, SignupRequest, TransferRequest, GenerateAddressResponse, SyncResponse, SignupResponse, ListAccountsResponse, TransferStatusRequest, TransferStatusResponse},
};

pub type Custody = Data<RwLock<CustodyService>>;

pub async fn sync_account<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    relayer_state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    relayer_state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        ServiceError::BadRequest(String::from("failed to parse account id"))
    })?;

    custody.sync_account(account_id, relayer_state);

    Ok(HttpResponse::Ok().json(SyncResponse{
        success: true
    }))
}
pub async fn account_info<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        ServiceError::BadRequest(String::from("failed to parse account id"))
    })?;

    let state = state.finalized.lock().unwrap();
    let relayer_index = state.next_index();

    let account_info = custody.account_info(account_id, relayer_index).ok_or(
        ServiceError::BadRequest(String::from("account with such id doesn't exist"))
    )?;

    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    _state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let mut custody = custody.write().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().json(SignupResponse{
        account_id: account_id.to_string()
    }))
}

pub async fn list_accounts<D: KeyValueDB>(
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        ServiceError::InternalError
    })?;

    let finalized = state.finalized.lock().unwrap();

    Ok(HttpResponse::Ok().json(ListAccountsResponse{
        accounts: custody.list_accounts(finalized.next_index()
    )}))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let request: TransferRequest = request.0.into();
    
    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        ServiceError::BadRequest(String::from("failed to parse account id"))
    })?;
    
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;
    
    custody.sync_account(account_id, state); // TODO: error handling

    let transaction_id = request.id.clone();
    if custody.get_job_id(&transaction_id)?.is_some() {
        return Err(ServiceError::BadRequest(String::from("transaction with such id already exists")));
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
            ServiceError::InternalError
        })?;

    let response = response.error_for_status().map_err(|e| {
        tracing::error!("relayer returned bad status code {:#?}", e);
        ServiceError::InternalError
    })?;

    let response:Response = response.json().await.map_err(|e| {
        tracing::error!("the relayer response was not JSON: {:#?}", e);
        ServiceError::InternalError
    })?;

    // TODO: multitransfer
    let nullifier = transaction_request[0].proof.inputs[1].bytes();
    let account = custody.account(account_id)?;
    account.save_nullifier(&transaction_id, nullifier).map_err(|_| {
        tracing::error!("failed to save nullifier");
        ServiceError::InternalError
    })?;

    custody.save_job_id(&transaction_id, &response.job_id).map_err(|_| {
        tracing::error!("failed to save job_id");
        ServiceError::InternalError
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
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let transaction_id = &request.transaction_id;
    let job_id = custody.get_job_id(transaction_id)?
        .ok_or(
            ServiceError::BadRequest(String::from("transaction with such id not found"))
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
            ServiceError::InternalError
        })?;

    let response: JobResponse = response.json().await.map_err(|e| {
        tracing::error!("the relayer response was not JSON: {:#?}", e);
        ServiceError::InternalError
    })?;
    
    Ok(HttpResponse::Ok().json(TransferStatusResponse{
        success: true,
        state: response.state,
        tx_hash: response.tx_hash,
        failed_reason: response.failed_reason,
    }))
}

pub async fn generate_shielded_address<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        ServiceError::BadRequest(String::from("failed to parse account id"))
    })?;

    let account = custody.account(account_id)?;
    let account = account.inner.read().map_err(|_| ServiceError::InternalError)?;
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
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        ServiceError::BadRequest(String::from("failed to parse account id"))
    })?;

    let account = custody.account(account_id)?;
    let txs = account.history(&state.pool).await;

    Ok(HttpResponse::Ok().json(
        txs 
    ))
}
