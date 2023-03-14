use actix_web::{
    web::{self, Data},
    HttpResponse,
};
use kvdb::KeyValueDB;
use libzkbob_rs::libzeropool::constants::OUT;
use serde::Deserialize;

use crate::{
    helpers::BytesRepr,
    state::State, types::job::JobStatus,
};

use super::ServiceError;

#[derive(Deserialize)]
pub struct TransactionsRequest {
    limit: u64,
    offset: u64,
}

pub async fn transactions<D: KeyValueDB>(
    request: web::Query<TransactionsRequest>,
    state: Data<State<D>>,
) -> Result<HttpResponse, ServiceError> {
    let (limit, offset) = (request.limit, request.offset);

    let offset = (offset / (OUT + 1) as u64) * (OUT + 1) as u64;
    let txs: Vec<_> = state.get_jobs(offset, limit)
        .map_err(|_| {ServiceError::InternalError})?
        .into_iter()
        .map(|job| {
            let prefix = if job.status == JobStatus::Done {
                "1"
            } else {
                "0"
            };
            let out_commit = hex::encode(job.commitment.bytes());
            let tx_hash = format!("{:#x}", job.transaction.unwrap().hash).replace("0x", "");
            let memo = hex::encode(job.memo);
            vec![prefix, &tx_hash, &out_commit, &memo].join("")
        })
        .collect();

    Ok(HttpResponse::Ok().json(txs))
}
