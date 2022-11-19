use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use tokio::sync::RwLock;
use std::{str::FromStr};
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

    let custody = custody.read().await;

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
    let mut custody = custody.write().await;

    let account_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn list_accounts<D: KeyValueDB>(
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;

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
    let request: TransferRequest = request.0.into();

    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    custody.sync_account(account_id, &state)?;

    let request_id = request
        .request_id
        .clone()
        .unwrap_or(Uuid::new_v4().to_string());
    if custody.has_request_id(&request_id)? {
        return Err(CustodyServiceError::DuplicateTransactionId);
    }

    let task = ScheduledTask {
        request_id: request_id.clone(),
        request,
        job_id: None,
        endpoint: None,
        retries_left: 42,
        status: TransferStatus::Proving,
        tx_hash: None,
        failure_reason: None
    };

    custody
        .sender
        .send(task.clone())
        .await
        .unwrap();
        
    custody.update_task_status(task.clone(), TransferStatus::Proving).await?;
        
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
        status:  TransferStatus::from(response.state),
        tx_hash: response.tx_hash,
        failure_reason: response.failed_reason,
    })
}
pub async fn transaction_status<D: KeyValueDB>(
    request: Query<TransferStatusRequest>,
    _: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;

    let request_id = request.0.request_id;
    let job_info = custody.get_job_info_by_request_id(&request_id)?.ok_or(
        CustodyServiceError::TransactionNotFound
    )?;

    let transaction_status = match job_info.job_id  {
        Some(id) => {
            let relayer_endpoint = custody.relayer_endpoint(&id)?;
            fetch_tx_status(&relayer_endpoint).await?
        },
        None => {
            TransactionStatusResponse {
                status: job_info.status,
                tx_hash: job_info.tx_hash,
                failure_reason: job_info.failure_reason,
            }
        }
    };

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

    let custody = custody.read().await;

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

    let custody = custody.read().await;

    custody.sync_account(account_id, &state)?;

    let account = custody.account(account_id)?;
    let txs = account
        .history(&state.pool, |nullifier: Vec<u8>| {
            custody.get_request_id(nullifier)
        })
        .await;

    Ok(HttpResponse::Ok().json(txs))
}
