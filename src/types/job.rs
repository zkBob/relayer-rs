use std::time::SystemTime;

use libzeropool::fawkes_crypto::{ff_uint::Num, engines::bn256::Fr};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use web3::types::Transaction as Web3Transaction;
use crate::helpers;

use super::transaction_request::TransactionRequest;

#[derive(Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum JobStatus {
    Created = 0, //Waiting for provers to get the task
    Proving = 1, //Generating tree update proofs
    Mining = 2,  //Waiting for tx receipt
    Done = 3,    //
    Rejected = 4,//This transaction or one of the preceeding tx in the queue were reverted
}

#[derive(Serialize,Deserialize, Debug)]
pub struct Response {
    #[serde(rename = "jobId")]
    pub job_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub created: SystemTime,
    pub status: JobStatus,
    pub transaction_request: Option<TransactionRequest>,
    pub transaction: Option<Web3Transaction>,
    pub index: u64,
    pub commitment: Num<Fr>,
    pub root: Option<Num<Fr>>,
    pub nullifier: Num<Fr>,
    pub memo: Vec<u8> // TODO: duplicates transaction_request.memo
}

impl Job {
    pub fn from_transaction_request(transaction_request: TransactionRequest) -> Job {
        let request_id = transaction_request
            .uuid
            .as_ref()
            .map(|id| Uuid::parse_str(&id).unwrap())
            .unwrap_or(Uuid::new_v4());

        let tx_type = u32::from_str_radix(&transaction_request.tx_type, 16).unwrap();
        let memo = hex::decode(&transaction_request.memo).unwrap();
        let nullifier = transaction_request.proof.inputs[1];
        let commitment = transaction_request.proof.inputs[2];
        let created = SystemTime::now();

        Job {
            id: request_id.as_hyphenated().to_string(),
            created,
            status: JobStatus::Created,
            transaction_request: Some(transaction_request),
            transaction: None,
            index: 0,
            commitment,
            nullifier,
            root: None,
            memo: helpers::truncate_memo_prefix(tx_type, memo),
        }
    }

    pub fn reject(mut self) -> () {
        self.status = JobStatus::Rejected;
    }
}