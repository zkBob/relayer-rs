use std::sync::Arc;

use actix_http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};

use libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{
        engines::Bn256,
        prover,
        verifier::{self, VK},
    },
    engines::bn256::Fr,
    ff_uint::Num,
};
use tokio::sync::mpsc::Sender;

use crate::configuration::ApplicationSettings;

#[derive(Debug)]
pub enum ServiceError {
    BadRequest(String),
    InternalError,
}

impl From<std::io::Error> for ServiceError {
    fn from(_: std::io::Error) -> Self {
        ServiceError::InternalError
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub proof: Proof,
    pub memo: String,
    tx_type: String,
    deposit_signature: String,
}

pub type TxRequest = Arc<Transaction>;

impl core::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("memo", &self.memo)
            .field("tx_type", &self.tx_type)
            .field("deposit_signature", &self.deposit_signature)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
pub struct Proof {
    pub inputs: Vec<Num<Fr>>,
    pub proof: prover::Proof<Bn256>,
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "request failed")
    }
}
impl ResponseError for ServiceError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            ServiceError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ServiceError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
pub async fn query() -> Result<HttpResponse, ServiceError> {
    Ok(HttpResponse::Ok().finish())
}

pub async fn transact(
    request: web::Json<Transaction>,
    sender: web::Data<Sender<TxRequest>>,
    vk: web::Data<VK<Bn256>>,
) -> Result<HttpResponse, ServiceError> {
    let transaction: Transaction = request.0.into();

    /*
    TODO:
    1. check nullifier for double spend

    2. check fee >= relayer fee

    */

    // check proof validity
    if !verifier::verify(
        vk.as_ref(),
        &transaction.proof.proof,
        &transaction.proof.inputs,
    ) {
        return Err(ServiceError::BadRequest("Invalid proof".to_owned()));
    }

    // this is actually Arc
    let copy = Arc::new(transaction);

    //TODO:  3 calculate new virtual state root

    // send to channel for further processing
    sender.send(copy).await.unwrap();

    //TODO:   4 generate UUID for request and save to in-memory map

    Ok(HttpResponse::Ok().finish())
}
