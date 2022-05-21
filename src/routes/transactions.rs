use actix_http::StatusCode;
use actix_web::{web::{self}, HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};

use libzeropool::{
    fawkes_crypto::{
        backend::bellman_groth16::{engines::Bn256, prover},
        engines::bn256::Fr,
        ff_uint::Num,
    },
    // native::boundednum::BoundedNum,
};

// use libzeropool::fawkes_crypto::backend::bellman_groth16::verifier::VK;
// use libzeropool::fawkes_crypto::backend::bellman_groth16::{verifier::verify, Parameters};
// use libzeropool::POOL_PARAMS;
// use libzeropool_rs::proof::prove_tx;

// use libzeropool_rs::client::{state::State, TxType, UserAccount};
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub enum ServiceError {
    BadRequest(String),
    InternalError,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    proof: Proof,
    pub memo: String,
    tx_type: String,
    deposit_signature: String,
}

// impl TryFrom<Json<Transaction>> for Transaction {
//     type Error = String;

//     fn try_from(value: Json<Transaction>) -> Result<Self, Self::Error> {
//         todo!()
//     }
    
// }

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
struct Proof {
    inputs: Vec<Num<Fr>>,
    proof: prover::Proof<Bn256>,
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
    sender: web::Data<Sender<Transaction>>,
) -> Result<HttpResponse, ServiceError> {

    let transaction:Transaction = request.0.into();

    tracing::info!("got http request {:#?}", transaction);

    sender.send(transaction).await.unwrap();

    Ok(HttpResponse::Ok().finish())
}
