use std::time::SystemTime;

use actix_web::{
    web::{self, Data},
    HttpResponse,
};
use kvdb::{DBKey, DBTransaction, KeyValueDB};
use serde::{Deserialize, Serialize};

use kvdb::DBOp::Insert;
use libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{engines::Bn256, prover, verifier},
    engines::bn256::Fr,
    ff_uint::Num,
};

use crate::{
    helpers::serialize,
    state::{Job, JobStatus, JobsDbColumn, State},
};
use memo_parser::{self, memo::Memo, memo::TxType};
use uuid::Uuid;

use super::ServiceError;
extern crate hex;

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
    #[serde(rename = "jobId")]
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

pub async fn query() -> Result<HttpResponse, ServiceError> {
    Ok(HttpResponse::Ok().finish())
}

pub async fn send_transactions<D: KeyValueDB>(
    request: web::Json<Vec<TransactionRequest>>,
    state: Data<State<D>>, // sender: web::Data<Sender<Data<Job>>>,
                           // vk: web::Data<VK<Bn256>>,
                           // web3_settings: web::Data<Web3Settings>,
                           // application_settings: web::Data<ApplicationSettings>,
) -> Result<HttpResponse, ServiceError> {
    let mut transaction_requests: Vec<TransactionRequest> = request.0.into();
    // TODO: support multitransfer
    let transaction_request = transaction_requests.pop().unwrap();
    transact(transaction_request, state).await
}

pub async fn send_transaction<D: KeyValueDB>(
    request: web::Json<TransactionRequest>,
    state: Data<State<D>>, // sender: web::Data<Sender<Data<Job>>>,
                           // vk: web::Data<VK<Bn256>>,
                           // web3_settings: web::Data<Web3Settings>,
                           // application_settings: web::Data<ApplicationSettings>,
) -> Result<HttpResponse, ServiceError> {
    let transaction_request: TransactionRequest = request.0.into();
    transact(transaction_request, state).await
}

pub async fn transact<D: KeyValueDB>(
    transaction_request: TransactionRequest,
    state: Data<State<D>>, // sender: web::Data<Sender<Data<Job>>>,
                           // vk: web::Data<VK<Bn256>>,
                           // web3_settings: web::Data<Web3Settings>,
                           // application_settings: web::Data<ApplicationSettings>,
) -> Result<HttpResponse, ServiceError> {
    let request_id = transaction_request
        .uuid
        .as_ref()
        .map(|id| Uuid::parse_str(&id).unwrap())
        .unwrap_or(Uuid::new_v4());

    // 1. check nullifier for double spend

    let nullifier = transaction_request.proof.inputs[1];
    tracing::info!(
        "request_id: {}, Checking Nullifier {:#?}",
        request_id,
        nullifier
    );
    let pool = &state.pool;
    if !pool.check_nullifier(&nullifier.to_string()).await.unwrap() {
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
    let parsed_memo = Memo::parse_memoblock(&tx_memo_bytes.unwrap(), tx_type);

    if parsed_memo.fee < state.web3.relayer_fee {
        tracing::warn!(
            "request_id: {}, fee {:#?} , Fee too low!",
            request_id,
            parsed_memo.fee
        );
        return Err(ServiceError::BadRequest("Fee too low!".to_owned()));
    }

    // 3. Check proof validity
    if !verifier::verify(
        &state.vk,
        &transaction_request.proof.proof,
        &transaction_request.proof.inputs,
    ) {
        tracing::info!("received bad proof");
        return Err(ServiceError::BadRequest("Invalid proof".to_owned()));
    }

    let commitment = transaction_request.proof.inputs[2];

    // send to channel for further processing
    let created = SystemTime::now();

    let nullifier_key = DBKey::from_slice(&serialize(nullifier).unwrap());

    let job = Job {
        id: request_id.as_hyphenated().to_string(),
        created,
        status: JobStatus::Created,
        transaction_request: Some(transaction_request),
        transaction: None,
        index: 0,
        commitment,
        nullifier,
        root: None,
    };

    state.jobs.write(DBTransaction {
        ops: vec![
            /*
            Saving Job info with transaction request to be later retrieved by client
            In case of rollback an existing row is mutated
            TODO: use Borsh instead of JSON
            */
            Insert {
                col: JobsDbColumn::Jobs as u32,
                key: DBKey::from_vec(request_id.as_bytes().to_vec()),
                value: serde_json::to_string(&job).unwrap().as_bytes().to_vec(),
            },
            /*
            Saving nullifiers to avoid double spend spam-atack.
            Nullifiers are stored to persistent DB, if rollback happens, they get deleted individually
             */
            Insert {
                col: JobsDbColumn::Nullifiers as u32,
                key: nullifier_key,
                value: vec![],
            },
        ],
    })?;
    state.sender.send(job).await.unwrap();

    let body = serde_json::to_string(&Response {
        job_id: request_id.as_hyphenated().to_string(),
    })
    .unwrap();
    Ok(HttpResponse::Ok().body(body))
}
