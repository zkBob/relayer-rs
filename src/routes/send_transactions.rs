use actix_web::{
    web::{self, Data},
    HttpResponse,
};
use kvdb::KeyValueDB;

use crate::{
    state::State,
    types::{
        job::{Job, Response},
        transaction_request::TransactionRequest,
    },
};

use super::ServiceError;
extern crate hex;


pub async fn query() -> Result<HttpResponse, ServiceError> {
    Ok(HttpResponse::Ok().finish())
}

pub async fn send_transactions<D: KeyValueDB>(
    request: web::Json<Vec<TransactionRequest>>,
    state: Data<State<D>>,
) -> Result<HttpResponse, ServiceError> {
    let mut transaction_requests: Vec<TransactionRequest> = request.0.into();
    // TODO: support multitransfer
    let transaction_request = transaction_requests.pop().unwrap();
    transact(transaction_request, state).await
}

pub async fn send_transaction<D: KeyValueDB>(
    request: web::Json<TransactionRequest>,
    state: Data<State<D>>,
) -> Result<HttpResponse, ServiceError> {
    let transaction_request: TransactionRequest = request.0.into();
    transact(transaction_request, state).await
}

pub async fn transact<D: KeyValueDB>(
    transaction_request: TransactionRequest,
    state: Data<State<D>>,
) -> Result<HttpResponse, ServiceError> {
    transaction_request.validate(&state).await?;

    let job = Job::from_transaction_request(transaction_request);
    let job_id = job.id.clone();
    state.save_new_job(&job)?;

    state.sender.send(job).await.unwrap();

    let body = serde_json::to_string(&Response { job_id }).unwrap();

    Ok(HttpResponse::Ok().body(body))
}
