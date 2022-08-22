use actix_web::{
    web::{self, Data},
    HttpResponse,
};
use kvdb::KeyValueDB;
use serde::Serialize;

use crate::state::State;

use super::ServiceError;

pub async fn job<D: KeyValueDB>(
    _path: web::Path<String>,
    _state: Data<State<D>>,
) -> Result<HttpResponse, ServiceError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct JobResponse {
        state: String,
        tx_hash: Option<Vec<String>>,
        failed_reason: Option<String>,
        created_on: u128,
        finished_on: u128,
    }

    let response = JobResponse {
        state: String::new(),
        tx_hash: None,
        failed_reason: None,
        created_on: 0,
        finished_on: 0,
    };

    Ok(HttpResponse::Ok().json(response))
}
