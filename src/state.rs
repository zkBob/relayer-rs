use std::{
    sync::Mutex,
    thread::sleep,
    time::{Duration, SystemTime},
};

use actix_web::web::{self, Data};
use kvdb::{DBOp::Insert, DBTransaction, KeyValueDB};
use libzeropool::{
    constants::OUT,
    fawkes_crypto::{
        backend::bellman_groth16::{engines::Bn256, verifier::VK},
        engines::bn256::Fr,
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
    configuration::Web3Settings, contracts::Pool, routes::send_transactions::TransactionRequest,
    tx_checker::check_tx, helpers,
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

#[derive(Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum JobStatus {
    Created = 0, //Waiting for provers to get the task
    Proving = 1, //Generating tree update proofs
    Mining = 2,  //Waiting for tx receipt
    Done = 3,    //
    Rejected = 4,//This transaction or one of the preceeding tx in the queue were reverted
}

pub enum JobsDbColumn {
    Jobs = 0,
    JobsIndex = 1,
    Nullifiers = 2,
    TxCheckTasks = 3 // Since we have KV store, we can't query job by status, iterating over all the rows is ineffective, 
                     //so we copy only those keys that require transaction receipt check
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
    pub fn reject(mut self) -> () {
        self.status = JobStatus::Rejected;
    }
}

pub struct State<D: 'static + KeyValueDB> {
    pub web3: Data<Web3Settings>,
    pub pending: DB<D>,
    pub jobs: Data<D>,
    pub finalized: DB<D>,
    pub vk: VK<Bn256>,
    pub pool: Pool,
    pub sender: Data<Sender<Job>>,
}

impl<D: 'static + KeyValueDB> State<D> {
    pub async fn sync(&self) -> Result<(), SyncError> {
        let mut finalized = self.finalized.lock().expect("failed to acquire lock");
        let mut pending = self.pending.lock().expect("failed to acquire lock");

        {
            let pool = &self.pool;
            let (contract_index, contract_root) = pool.root().await?;
            let local_finalized_root = finalized.get_root();
            let local_finalized_index = finalized.next_index();
            tracing::debug!("local root {}", local_finalized_root.to_string());
            tracing::debug!("contract root {}", contract_root.to_string());

            if !local_finalized_root.eq(&contract_root) {
                let missing_indices: Vec<u64> = (local_finalized_index..contract_index.as_u64())
                    .step_by(OUT + 1)
                    .into_iter()
                    .map(|i| local_finalized_index + (i + 1) * (OUT as u64 + 1))
                    .collect();
                tracing::debug!("mising indices: {:?}", missing_indices);

                let events = pool
                    .get_events(Some(BlockNumber::Earliest), Some(BlockNumber::Latest), None)
                    .await
                    .unwrap();

                std::fs::write(
                    "tests/data/events.json",
                    serde_json::to_string(&events).unwrap(),
                )
                .unwrap();
                for event in events.iter() {
                    let index = event.event_data.0.as_u64() - (OUT + 1) as u64;
                    if let Some(tx_hash) = event.transaction_hash {
                        if let Some(tx) = pool.get_transaction(tx_hash).await.unwrap() {
                            let calldata = memoparser::parse_calldata(&tx.input.0, None)
                                .expect("Calldata is invalid!");
                            
                            let commitment = Num::from_uint_reduced(NumRepr(
                                Uint::from_big_endian(&calldata.out_commit),
                            ));
                            tracing::debug!("index: {}, commit {}", index, commitment.to_string());
                            finalized.add_leafs_and_commitments(vec![], vec![(index, commitment)]);

                            if pending.next_index() <= index {
                                pending
                                    .add_leafs_and_commitments(vec![], vec![(index, commitment)]);
                            }

                            tracing::debug!(
                                "root:\n\tpending:{:#?}\n\tfinalized:{:#?}",
                                pending.get_root().to_string(),
                                finalized.get_root().to_string()
                            );

                            use kvdb::DBKey;

                            let nullifier: Num<Fr> = Num::from_uint_reduced(NumRepr(
                                Uint::from_big_endian(&calldata.nullifier),
                            ));

                            // TODO: fix this
                            let memo = Vec::from(&tx.input.0[644..(644 + calldata.memo_size) as usize]);

                            let job_id = Uuid::new_v4();

                            let job = Job {
                                id: job_id.as_hyphenated().to_string(),
                                created: SystemTime::now(),
                                status: JobStatus::Done,
                                transaction_request: None,
                                transaction: Some(tx),
                                index,
                                commitment,
                                nullifier,
                                root: None,
                                memo: helpers::truncate_memo_prefix(calldata.tx_type, memo)
                            };

                            tracing::debug!("writing tx hash {:#?}", hex::encode(tx_hash));
                            let db_transaction = DBTransaction {
                                ops: vec![
                                    Insert {
                                        col: JobsDbColumn::Jobs as u32,
                                        key: DBKey::from_vec(
                                            job_id.as_hyphenated().to_string().as_bytes().to_vec(),
                                        ),
                                        value: serde_json::to_vec(&job).unwrap(),
                                    },
                                    Insert { 
                                        col: JobsDbColumn::JobsIndex as u32,
                                        key: DBKey::from_slice(&job.index.to_be_bytes()),
                                        value: job_id.as_hyphenated().to_string().as_bytes().to_vec(),
                                    }
                                ],
                            };
                            self.jobs.write(db_transaction)?;
                        }
                    }
                }
            }

            tracing::info!(
                "local finalized root after sync {:#?}, index : {}",
                finalized.get_root().to_string(),
                finalized.next_index()
            );
        }

        Ok(())
    }

    pub fn start_poller(self) -> () {
        let jobs: Vec<Job> = self
            .jobs
            .iter(JobsDbColumn::TxCheckTasks as u32)
            .map(|(_k, v)| serde_json::from_slice(&v[..]).unwrap())
            .collect();
        tokio::spawn(async move {
            let state = Data::new(self);
            for job in jobs.into_iter() {
                let state = state.clone();
                tracing::debug!("polling started at {:?}", SystemTime::now());
                match check_tx(job, state).await {
                    Err(_) | Ok(JobStatus::Rejected) => {
                        tracing::warn!(
                            "caught error / revert " // TODO: add job id
                        );
                        break;
                    }
                    _ => (),
                }
            }
            sleep(Duration::from_secs(5));
        });
    }
}
