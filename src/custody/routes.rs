use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use std::{str::FromStr, sync::RwLock};
use uuid::Uuid;

use crate::{
    custody::types::TransferResponse, helpers::BytesRepr, routes::job::JobResponse, state::State,
    types::job::Response,
};

use super::{
    errors::CustodyServiceError,
    service::CustodyService,
    types::{
        AccountInfoRequest, GenerateAddressResponse, ListAccountsResponse, SignupRequest,
        SignupResponse, TransactionStatusResponse, TransferRequest, TransferStatusRequest,
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

    custody.sync_account(account_id, &state)?;

    let account_info = custody
        .account_info(account_id)
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

    Ok(HttpResponse::Ok().json(custody.list_accounts()))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let mut request: TransferRequest = request.0.into();

    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        CustodyServiceError::CustodyLockError
    })?;

    custody.sync_account(account_id, &state)?;

    let request_id = request
        .request_id
        .clone()
        .unwrap_or(Uuid::new_v4().to_string());
    if custody.get_job_id_by_request_id(&request_id)?.is_some() {
        return Err(CustodyServiceError::DuplicateTransactionId);
    }

    request.request_id = Some(request_id.clone());

    custody
        .sender
        .send(request)
        .await
        .map_err(|e| CustodyServiceError::CustodyLockError);

    Ok(HttpResponse::Ok().json(TransferResponse {
        request_id: request_id.clone(),
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

    let transaction_id = &request.request_id;
    let job_id = custody
        .get_job_id_by_request_id(transaction_id)?
        .ok_or(CustodyServiceError::TransactionNotFound)?;

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

    Ok(HttpResponse::Ok().json(TransactionStatusResponse {
        state: response.state,
        tx_hash: response.tx_hash,
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

    custody.sync_account(account_id, &state)?;

    let account = custody.account(account_id)?;
    let txs = account
        .history(&state.pool, |nullifier: Vec<u8>| {
            custody.get_request_id(nullifier)
        })
        .await;

    Ok(HttpResponse::Ok().json(txs))
}
