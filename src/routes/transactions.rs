use std::time::SystemTime;

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
        verifier,
    },
    engines::bn256::Fr,
    ff_uint::Num,
};

use crate::{
    state::{Job, State, JobStatus},
};
use uuid::Uuid;
use memo_parser::{self, memo::Memo, memo::TxType};
extern crate hex;
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
pub struct TransactionRequest {
    pub uuid: Option<String>,
    pub proof: Proof,
    pub memo: String,
    pub tx_type: String,
    pub deposit_signature: String,
}


#[derive(Serialize)]
pub struct Response {
    job_id: String,
}

impl core::fmt::Debug for TransactionRequest {
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
    request: web::Json<TransactionRequest>,
    state: Data<State<D>>, // sender: web::Data<Sender<Data<Job>>>,
                           // vk: web::Data<VK<Bn256>>,
                           // web3_settings: web::Data<Web3Settings>,
                           // application_settings: web::Data<ApplicationSettings>,
) -> Result<HttpResponse, ServiceError> {
    let transaction_request: TransactionRequest = request.0.into();

    let request_id = transaction_request
        .uuid
        .as_ref()
        .map(|id| Uuid::parse_str(&id).unwrap())
        .unwrap_or(Uuid::new_v4());

    // 1. check nullifier for double spend

    let nullifier = transaction_request.proof.inputs[1].to_string();
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
        return Err(ServiceError::BadRequest(error_message));
    }

    // 2. check fee >= relayer fee TODO: parse memo properly

    let tx_memo_bytes = hex::decode(&transaction_request.memo);
    if tx_memo_bytes.is_err() {
        return Err(ServiceError::BadRequest("Bad memo field!".to_owned()));
    }
    let tx_type = match transaction_request.tx_type.as_str() {
        "0000" => TxType::Deposit,
        "0001" => TxType::Transfer,
        "0002" => TxType::Withdrawal,
        "0003" => TxType::DepositPermittable,
        _ => TxType::Deposit,
    };
    let parsed_memo = Memo::parse_memoblock(tx_memo_bytes.unwrap(), tx_type);
    if parsed_memo.fee < state.web3.relayer_fee {
        tracing::warn!(
            "request_id: {}, fee {:#?} , Fee too low!",
            request_id,
            parsed_memo.fee
        );
        return Err(ServiceError::BadRequest("Fee too low!".to_owned()));
    }

    // check proof validity

    // 3. Check proof
    if !verifier::verify(
        &state.vk,
        &transaction_request.proof.proof,
        &transaction_request.proof.inputs,
    ) {
        tracing::info!("received bad proof");
        return Err(ServiceError::BadRequest("Invalid proof".to_owned()));
    }

    //TODO:  3 calculate new virtual state root
    let mut pending_tree = state.pending.lock().unwrap();
    pending_tree.append_hash(transaction_request.proof.inputs[2], false);
    let virtual_state_root = pending_tree.get_root();

    // send to channel for further processing
    let created = SystemTime::now();
    let job = Job {
        created,
        status: JobStatus::Created,
        transaction_request: Some(transaction_request),
        transaction: None
    };

    state.jobs.write(DBTransaction {
        ops: vec![Insert {
            col: 0,
            key: DBKey::from_vec(request_id.as_bytes().to_vec()),
            value: serde_json::to_string(&job).unwrap().as_bytes().to_vec(),
        }],
    })?;
    state.sender.send(Data::new(job)).await.unwrap();

    //TODO:   4 generate UUID for request and save to in-memory map

    let body = serde_json::to_string(&Response {
        job_id: request_id.as_hyphenated().to_string(),
    })
    .unwrap();
    Ok(HttpResponse::Ok().body(body))
}
