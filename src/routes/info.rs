use actix_web::{web::Data, HttpResponse};
use kvdb::KeyValueDB;
use serde::Serialize;

use crate::state::State;

use super::ServiceError;

pub async fn info<D: KeyValueDB>(state: Data<State<D>>) -> Result<HttpResponse, ServiceError> {
    #[derive(Serialize)]
    struct InfoResponse {
        root: String,
        #[serde(rename = "optimisticRoot")]
        optimistic_root: String,
        #[serde(rename = "deltaIndex")]
        delta_index: u64,
        #[serde(rename = "optimisticDeltaIndex")]
        optimistic_delta_index: u64,
    }

    let (root, delta_index) = {
        let finalized = state.finalized.lock().unwrap();
        let root = finalized.get_root().to_string();
        let next_index = finalized.next_index();
        (root, next_index)
    };

    let (optimistic_root, optimistic_delta_index) = {
        let pending = state.pending.lock().unwrap();
        let root = pending.get_root().to_string();
        let next_index = pending.next_index();
        (root, next_index)
    };

    Ok(HttpResponse::Ok().json(InfoResponse {
        root,
        optimistic_root,
        delta_index,
        optimistic_delta_index,
    }))
}
