use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use actix_http::StatusCode;
use actix_web::{
    web::{self, Data},
    HttpResponse, ResponseError,
};
use kvdb::{DBKey, DBTransaction, KeyValueDB};
use serde::{Deserialize, Serialize};

use kvdb::DBOp::Insert;
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

use crate::startup::JobStatus;

use crate::{
    configuration::{ApplicationSettings, Web3Settings},
    contracts::Pool,
    startup::{Job, State},
};
use uuid::Uuid;
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
    pub uuid: Option<String>,
    pub proof: Proof,
    pub memo: String,
    tx_type: String,
    deposit_signature: String,
}

pub type TxRequest = Arc<Job>;

#[derive(Serialize)]
pub struct Response {
    job_id: String,
}

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

    let request_id = transaction
        .uuid
        .as_ref()
        .map(|id| Uuid::parse_str(&id).unwrap())
        .unwrap_or(Uuid::new_v4());

    // 1. check nullifier for double spend

    let nullifier = transaction.proof.inputs[1].to_string();
    tracing::info!(
        "request_id: {}, Checking Nullifier {:#?}",
        request_id,
        nullifier
    );
    let pool = &state.pool;
    if !pool.check_nullifier(&nullifier).await.unwrap() {
        let error_message = format!(
            "request_id: {}, Nullifier {:#?} , Double spending detected",
            request_id, &nullifier
        );
        tracing::warn!("{}", error_message);
        // return Err(ServiceError::BadRequest(error_message));
    }

    // 2. check fee >= relayer fee TODO: parse memo properly

    // let fee = from_str::<u64>(&transaction.memo[0..8]).unwrap();
    // if fee <= state.web3.relayer_fee {
    //     tracing::warn!(
    //         "request_id: {}, Nullifier {:#?} , Double spending detected",
    //         request_id,
    //         nullifier
    //     );
    //     return Err(ServiceError::BadRequest("Fee too low!".to_owned()));
    // }

    // check proof validity

    // 3. Check proof
    if !verifier::verify(
        &state.vk,
        &transaction.proof.proof,
        &transaction.proof.inputs,
    ) {
        tracing::info!("received bad proof");
        return Err(ServiceError::BadRequest("Invalid proof".to_owned()));
    }

    //TODO:  3 calculate new virtual state root

    // send to channel for further processing
    let created = SystemTime::now();
    let job = Job {
        created,
        status: JobStatus::Created,
        transaction,
    };

    let a = serde_json::to_string(&job).unwrap().as_bytes().to_vec();

    state.jobs.write(DBTransaction {
        ops: vec![Insert {
            col: 0,
            key: DBKey::from_vec(request_id.as_bytes().to_vec()),
            value: a,
        }],
    })?;
    state.sender.send(TxRequest::new(job)).await.unwrap();

    //TODO:   4 generate UUID for request and save to in-memory map

    let body = serde_json::to_string(&Response {
        job_id: request_id.as_hyphenated().to_string(),
    })
    .unwrap();
    Ok(HttpResponse::Ok().body(body))
}
