use std::{sync::Mutex, time::SystemTime};

use actix_web::web::{self, Data};
use kvdb::{DBOp::Insert, DBTransaction, KeyValueDB};
use libzeropool::{
    constants::OUT,
    fawkes_crypto::{
        backend::bellman_groth16::{engines::Bn256, verifier::VK},
        ff_uint::{Num, NumRepr, Uint},
    },
    native::params::PoolBN256,
};
use libzkbob_rs::merkle::MerkleTree;
use memo_parser::memoparser;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;
use web3::types::{BlockNumber, Transaction as Web3Transaction};

use crate::{
    configuration::Web3Settings, contracts::Pool, routes::transactions::TransactionRequest,
};

pub type DB<D> = web::Data<Mutex<MerkleTree<D, PoolBN256>>>;

#[derive(Debug)]
pub enum SyncError {
    BadAbi(std::io::Error),
    GeneralError(String),
    ContractException(web3::contract::Error),
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::BadAbi(e)
    }
}

impl From<web3::contract::Error> for SyncError {
    fn from(e: web3::contract::Error) -> Self {
        SyncError::ContractException(e)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum JobStatus {
    Created,
    Proving,
    Mining,
    Done,
    Rejected,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub created: SystemTime,
    pub status: JobStatus,
    pub transaction_request: Option<TransactionRequest>,
    pub transaction: Option<Web3Transaction>,
}

pub struct State<D: 'static + KeyValueDB> {
    pub web3: Data<Web3Settings>,
    pub pending: DB<D>,
    pub jobs: Data<D>,
    pub finalized: DB<D>,
    pub vk: VK<Bn256>,
    pub pool: Pool,
    pub sender: Data<Sender<Data<Job>>>, // rx: Receiver<Transaction>,
}

impl<D: 'static + KeyValueDB> State<D> {
    pub async fn sync(&self) -> Result<(), SyncError> {
        let mut db = self.finalized.lock().expect("failed to acquire lock");

        {
            let pool = &self.pool;
            let (contract_index, contract_root) = pool.root().await?;
            let local_root = db.get_root();
            let local_index = db.next_index();
            tracing::debug!("local root {}", local_root.to_string());
            tracing::debug!("contract root {}", contract_root.to_string());

            if !local_root.eq(&contract_root) {
                let missing_indices: Vec<u64> = (local_index..contract_index.as_u64())
                    .step_by(OUT + 1)
                    .into_iter()
                    .map(|i| local_index + (i + 1) * (OUT as u64 + 1))
                    .collect();
                tracing::debug!("mising indices: {:?}", missing_indices);

                for event in pool
                    .get_events(Some(BlockNumber::Earliest), Some(BlockNumber::Latest), None)
                    .await
                    .unwrap()
                    .iter()
                {
                    let index = event.event_data.0 - 128;
                    if let Some(tx_hash) = event.transaction_hash {
                        if let Some(tx) = pool.get_transaction(tx_hash).await.unwrap() {
                            let calldata = &tx.input.0;
                            let calldata = memoparser::parse_calldata(hex::encode(calldata), None)
                                .expect("Calldata is invalid!");

                            let commit = Num::from_uint_reduced(NumRepr(Uint::from_big_endian(
                                &calldata.out_commit,
                            )));
                            tracing::debug!("index: {}, commit {}", index, commit.to_string());
                            db.add_leafs_and_commitments(vec![], vec![(index.as_u64(), commit)]);
                            tracing::debug!("local root {:#?}", db.get_root().to_string());

                            use kvdb::DBKey;

                            let job_id = Uuid::new_v4();
                            
                            let job = Job {
                                id: job_id.as_hyphenated().to_string(),
                                created: SystemTime::now(),
                                status: JobStatus::Done,
                                transaction_request: None,
                                transaction: Some(tx),
                            };

                            let db_transaction = DBTransaction {
                                ops: vec![Insert {
                                    col: 0,
                                    key: DBKey::from_vec(job_id.as_bytes().to_vec()),
                                    value: serde_json::to_vec(&job).unwrap(),
                                }],
                            };
                            self.jobs.write(db_transaction)?;
                        }
                    }
                }
            }

            tracing::debug!("local root after sync {:#?}", db.get_root().to_string());
        }

        Ok(())
    }
}
