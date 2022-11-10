use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use std::{str::FromStr, sync::RwLock};
use uuid::Uuid;

use crate::{routes::ServiceError, state::State, types::job::Response};

use super::{
    service::CustodyService,
    types::{AccountInfoRequest, SignupRequest, TransferRequest, GenerateAddressResponse},
};

// pub type Custody = Data<CustodyService>;
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

    let account_id = Uuid::from_str(&request.id).unwrap();

    custody.sync_account(account_id, relayer_state);

    Ok(HttpResponse::Ok().finish())
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

    let account_id = Uuid::from_str(&request.id).unwrap();

    let state = state.finalized.lock().unwrap();
    let relayer_index = state.next_index();

    custody
        .account_info(account_id, relayer_index)
        .map_or(Ok(HttpResponse::NotFound().finish()), |res| {
            Ok(HttpResponse::Ok().json(res))
        })
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
    let acc_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().body(acc_id.to_string()))
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

    Ok(HttpResponse::Ok().json(custody.list_accounts(finalized.next_index())))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, ServiceError> {
    let request: TransferRequest = request.0.into();
    let account_id = Uuid::from_str(&request.account_id).unwrap();
    let custody = custody.read().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;
    custody.sync_account(account_id, state); // TODO: error handling

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

    tracing::info!("relayer returned the job id: {:#?}", response.job_id );

    Ok(HttpResponse::Ok().json(response))
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
        tracing::error!("failed to parse account_id");
        ServiceError::BadRequest(err.to_string())
    })?;

    let account = custody.account(account_id)?;
    let account = account.inner.read().map_err(|_| ServiceError::InternalError)?;
    let address = account.generate_address();

    Ok(HttpResponse::Ok().json(GenerateAddressResponse{
        success: true,
        address,
    }))
}
