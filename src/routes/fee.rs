use actix_web::{web::Data, HttpResponse};
use kvdb::KeyValueDB;
use serde::Serialize;

use crate::state::State;

use super::ServiceError;

pub async fn fee<D: KeyValueDB>(state: Data<State<D>>) -> Result<HttpResponse, ServiceError> {
    #[derive(Serialize)]
    struct FeeResponse {
        fee: String,
    }

    Ok(HttpResponse::Ok().json(FeeResponse {
        fee: state.web3.relayer_fee.to_string(),
    }))
}
