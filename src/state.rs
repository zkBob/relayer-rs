use std::{sync::Mutex, time::{SystemTime, Duration}, thread::sleep};

use actix_web::web::{self, Data};
use ethabi::ethereum_types::U256;
use kvdb::{DBOp::Insert, DBTransaction, KeyValueDB};
use libzeropool::{
    constants::{OUT, OUTPLUSONELOG},
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
    configuration::Web3Settings, contracts::Pool, routes::transactions::TransactionRequest, tx_checker::check_tx,
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

pub enum JobsDbColumn {
    Jobs = 0,
    Nullifiers = 1,
    TxCheckTasks = 2,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub created: SystemTime,
    pub status: JobStatus,
    pub transaction_request: Option<TransactionRequest>,
    pub transaction: Option<Web3Transaction>,
    pub index: u64,
    pub commitment: Num<Fr>,
    pub root: Option<Num<Fr>>,
    pub nullifier: Num<Fr>,
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
                    let index = event.event_data.0.as_u64() - (OUT + 1) as u64;
                    if let Some(tx_hash) = event.transaction_hash {
                        if let Some(tx) = pool.get_transaction(tx_hash).await.unwrap() {
                            let calldata = memoparser::parse_calldata(&tx.input.0, None)
                                .expect("Calldata is invalid!");

                            let commitment = Num::from_uint_reduced(NumRepr(
                                Uint::from_big_endian(&calldata.out_commit),
                            ));
                            tracing::debug!("index: {}, commit {}", index, commitment.to_string());
                            db.add_leafs_and_commitments(vec![], vec![(index, commitment)]);
                            tracing::debug!("local root {:#?}", db.get_root().to_string());

                            use kvdb::DBKey;

                            let nullifier:Num<Fr> = Num::from_uint_reduced(NumRepr(
                                Uint::from_big_endian(&calldata.nullifier),
                            ));

                            let job = Job {
                                created: SystemTime::now(),
                                status: JobStatus::Done,
                                transaction_request: None,
                                transaction: Some(tx),
                                index,
                                commitment,
                                nullifier,
                                root: None,
                            };


                            tracing::debug!("writing tx hash {:#?}", hex::encode(tx_hash) );
                            let db_transaction = DBTransaction {
                                ops: vec![Insert {
                                    col: JobsDbColumn::Jobs as u32,
                                    key: DBKey::from_vec(tx_hash.as_bytes().to_vec()),
                                    value: serde_json::to_vec(&job).unwrap(),
                                }],
                            };
                            self.jobs.write(db_transaction)?;

                            //TODO: state.addTx(index, Buffer.from(commitAndMemo, 'hex'))
                        }
                    }
                }
            }

            tracing::debug!("local root after sync {:#?}", db.get_root().to_string());
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
