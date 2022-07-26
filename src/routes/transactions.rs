use std::sync::Arc;

use actix_http::StatusCode;
use actix_web::{
    web::{self, Data},
    HttpResponse, ResponseError,
};
use kvdb::KeyValueDB;
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
use serde_json::from_str;
use tokio::sync::mpsc::Sender;

use crate::{
    configuration::{ApplicationSettings, Web3Settings},
    contracts::Pool,
    startup::State,
};
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

pub async fn transact<D: KeyValueDB>(
    request: web::Json<Transaction>,
    state: Data<State<D>>, // sender: web::Data<Sender<TxRequest>>,
                           // vk: web::Data<VK<Bn256>>,
                           // web3_settings: web::Data<Web3Settings>,
                           // application_settings: web::Data<ApplicationSettings>,
) -> Result<HttpResponse, ServiceError> {
    let transaction: Transaction = request.0.into();
    /*
    TODO:
    1. check nullifier for double spend

    2. check fee >= relayer fee

    */
    let nullifier = transaction.proof.inputs[1].to_string();
    tracing::info!("Checking Nullifier {:#?}", nullifier);
    let pool = &state.pool;
    if !pool.check_nullifier(&nullifier).await.unwrap() {

        tracing::warn!("Nullifier {:#?} , Double spending detected", nullifier);
        return Err(ServiceError::BadRequest(
            "Double spending detected!".to_owned(),
        ));
    }

    let fee = from_str::<u64>(&transaction.memo[0..8]).unwrap();
    if fee <= state.web3.relayer_fee {
        return Err(ServiceError::BadRequest("Fee too low!".to_owned()));
    }

    // check proof validity
    if !verifier::verify(
        &state.vk,
        &transaction.proof.proof,
        &transaction.proof.inputs,
    ) {
        tracing::info!("received bad proof");
        return Err(ServiceError::BadRequest("Invalid proof".to_owned()));
    }

    // this is actually Arc
    let copy = Arc::new(transaction);

    //TODO:  3 calculate new virtual state root

    // send to channel for further processing
    state.sender.send(copy).await.unwrap();

    //TODO:   4 generate UUID for request and save to in-memory map

    Ok(HttpResponse::Ok().finish())
}
