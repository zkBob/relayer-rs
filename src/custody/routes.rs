use std::sync::Mutex;

use actix_web::{web::{Query, Data, Json}, HttpResponse};
use kvdb::KeyValueDB;
use uuid::Uuid;
use std::str::FromStr;

use crate::{state::State, routes::ServiceError};

use super::{types::{AccountInfoRequest, SignupRequest}, service::CustodyService};

pub async fn sync_account<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    relayer_state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).unwrap();

    custody.sync_account(account_id, relayer_state);

    Ok(HttpResponse::Ok().finish())
}
pub async fn account_sync_status<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).unwrap();

    let state = state.finalized.lock().unwrap();
    let relayer_index = state.next_index();

    custody
        .sync_status_inner(account_id, relayer_index)
        .map_or(Ok(HttpResponse::NotFound().finish()), |res| {
            Ok(HttpResponse::Ok().json(res))
        })
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    _state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let mut custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;
    let acc_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().body(acc_id.to_string()))
}

pub async fn list_accounts<D: KeyValueDB>(
    state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let finalized = state.finalized.lock().unwrap();

    Ok(HttpResponse::Ok().json(custody.list_accounts(finalized.next_index())))
}