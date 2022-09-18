use std::time::SystemTime;

use actix_web::{
    web::{self, Data},
    HttpResponse,
};
use kvdb::KeyValueDB;
use serde::Serialize;
use uuid::Uuid;

use crate::{state::{State, JobsDbColumn}, types::job::{Job, JobStatus}};

use super::ServiceError;

pub async fn job<D: KeyValueDB>(
    path: web::Path<String>,
    state: Data<State<D>>,
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

    let job_id = path.into_inner();
    let job_id = Uuid::parse_str(&job_id)
        .map_err(|_| ServiceError::BadRequest(String::from("failed to parse job id")))?;

    let job = state
        .jobs
        .get(
            JobsDbColumn::Jobs as u32,
            job_id.as_hyphenated().to_string().as_bytes(),
        )
        .map_err(|_| ServiceError::BadRequest(String::from("job with such id not found")))?
        .ok_or(ServiceError::BadRequest(String::from("job with such id not found")))?;
    let job: Job = serde_json::from_slice(&job).unwrap();

    let state = match job.status {
        JobStatus::Created | JobStatus::Proving | JobStatus::Mining => "active",
        JobStatus::Done => "completed",
        JobStatus::Rejected => "failed"        
    };

    let mut response = JobResponse {
        state: String::from(state),
        tx_hash: None,
        failed_reason: None,
        created_on: job.created.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis(),
        finished_on: 0, // TODO: Add finished in job //job.finished.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
    };

    if job.transaction.is_some() {
        response.tx_hash = Some(vec![format!("{:#x}", job.transaction.unwrap().hash)]);
    }

    if job.status == JobStatus::Rejected {
        response.failed_reason = Some(String::new()); // TODO: save fail reason
    }

    Ok(HttpResponse::Ok().json(response))
}
