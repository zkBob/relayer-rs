use actix_web::{
    web::{self, Data},
    HttpResponse,
};
use kvdb::KeyValueDB;
use libzeropool::constants::OUT;
use serde::Deserialize;

use crate::{
    helpers::BytesRepr,
    state::{Job, JobStatus, JobsDbColumn, State},
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

    let mut offset = (offset / (OUT + 1) as u64) * (OUT + 1) as u64;
    let mut txs = vec![];
    for _ in 0..limit {
        let job_id = state
            .jobs
            .get(JobsDbColumn::JobsIndex as u32, &offset.to_be_bytes());
        if let Ok(Some(job_id)) = job_id {
            let job = state.jobs.get(JobsDbColumn::Jobs as u32, &job_id);
            if let Ok(Some(job)) = job {
                let job: Job = serde_json::from_slice(&job).unwrap();
                if job.transaction.is_none() {
                    break;
                }
                let prefix = if job.status == JobStatus::Done {
                    "1"
                } else {
                    "0"
                };
                let out_commit = hex::encode(job.commitment.bytes());
                let tx_hash = format!("{:#x}", job.transaction.unwrap().hash).replace("0x", "");
                let memo = hex::encode(job.memo);
                txs.push(vec![prefix, &tx_hash, &out_commit, &memo].join(""));
                offset += (OUT + 1) as u64;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok(HttpResponse::Ok().json(txs))
}
