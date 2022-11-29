use std::{cmp::min, io, time::SystemTime};

use actix_web::web::{self, Data};
use ethabi::ethereum_types::U64;
use kvdb::{DBKey, DBOp::Insert, DBTransaction, KeyValueDB};
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
use tokio::sync::{mpsc::Sender, Mutex};
use tracing_futures::Instrument;
use uuid::Uuid;
use web3::types::BlockNumber;

use crate::{
    configuration::Settings,
    contracts::Pool,
    helpers,
    types::job::{Job, JobStatus},
};

pub type DB<D> = web::Data<Mutex<MerkleTree<D, PoolBN256>>>;

#[derive(Debug)]
pub enum SyncError {
    BadAbi(std::io::Error),
    GeneralError(String),
    ContractException(web3::contract::Error),
    RpcNodeUnavailable,
    Web3Error(web3::Error),
    RequestTimeout(tokio::time::error::Elapsed),
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

impl From<web3::Error> for SyncError {
    fn from(e: web3::Error) -> Self {
        SyncError::Web3Error(e)
    }
}

impl From<tokio::time::error::Elapsed> for SyncError {
    fn from(e: tokio::time::error::Elapsed) -> Self {
        SyncError::RequestTimeout(e)
    }
}

pub enum JobsDbColumn {
    Jobs = 0,
    JobsIndex = 1,
    Nullifiers = 2,
    TxCheckTasks = 3, // Since we have KV store, we can't query job by status, iterating over all the rows is ineffective,
    //so we copy only those keys that require transaction receipt check
    SyncedBlockIndex = 4,
}

pub struct State<D: 'static + KeyValueDB> {
    pub settings: Data<Settings>,
    pub pending: DB<D>,
    pub jobs: Data<D>,
    pub finalized: DB<D>,
    pub vk: VK<Bn256>,
    pub pool: Pool,
    pub sender: Data<Sender<Job>>,
}

impl<D: 'static + KeyValueDB> State<D> {
    pub async fn sync(&self) -> Result<(), SyncError> {
        let mut finalized = self.finalized.lock().await;
        let mut pending = self.pending.lock().await;

        {
            let pool = &self.pool;
            let (contract_index, contract_root) = pool.root().await?;
            let local_finalized_root = finalized.get_root();
            let local_finalized_index = finalized.next_index();
            tracing::info!(
                "relayer state sync, checking indices: local={} contract={} ",
                local_finalized_index,
                contract_index,
            );

            if !local_finalized_root.eq(&contract_root) {

                let from_block = self.get_from_block();
                let to_block = pool.block_number().await?;

                let batch_size = self.settings.web3.batch_size;
                let mut start_block = from_block.as_u64();
                let finish_block = to_block.as_u64();
                let mut batch_end_block = start_block + batch_size;
                tracing::info!(
                    "starting sync from  block  #{} to block #{}",
                    start_block,
                    finish_block
                );

                while start_block < finish_block {
                    let progress = ((start_block - from_block.as_u64()) * 100)
                        / (finish_block - from_block.as_u64());

                    let events = pool
                        .get_events(
                            Some(BlockNumber::Number(U64::from(start_block))),
                            Some(BlockNumber::Number(U64::from(batch_end_block))),
                            None,
                        )
                        .instrument(tracing::debug_span!("getting events from contract"))
                        .await?;

                    tracing::info!(
                        "batch {} - {}, events: {}, progress: {}",
                        start_block,
                        batch_end_block,
                        events.len(),
                        progress,
                    );
                    for event in events.iter() {
                        let index = event.event_data.0.as_u64() - (OUT + 1) as u64;
                        if let Some(tx_hash) = event.transaction_hash {
                            if let Some(tx) = pool
                                .get_transaction(tx_hash)
                                .instrument(tracing::debug_span!(
                                    "getting transaction by hash",
                                    hash = tx_hash.to_string()
                                ))
                                .await
                                .map_err(|_| {
                                    SyncError::RpcNodeUnavailable
                                })?
                            {
                                let calldata = memoparser::parse_calldata(&tx.input.0, None)
                                    .expect("Calldata is invalid!");

                                let commitment = Num::from_uint_reduced(NumRepr(
                                    Uint::from_big_endian(&calldata.out_commit),
                                ));
                                tracing::debug!(
                                    "index: {}, commit {}",
                                    index,
                                    commitment.to_string()
                                );
                                finalized
                                    .add_leafs_and_commitments(vec![], vec![(index, commitment)]);

                                if pending.next_index() <= index {
                                    pending.add_leafs_and_commitments(
                                        vec![],
                                        vec![(index, commitment)],
                                    );
                                }

                                tracing::debug!(
                                    "saved commitment at index = {}",
                                    finalized.next_index()
                                );

                                let nullifier: Num<Fr> = Num::from_uint_reduced(NumRepr(
                                    Uint::from_big_endian(&calldata.nullifier),
                                ));

                                // TODO: fix this
                                let memo = Vec::from(
                                    &tx.input.0[644..(644 + calldata.memo_size) as usize],
                                );

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
                                    memo: helpers::truncate_memo_prefix(calldata.tx_type, memo),
                                };

                                tracing::debug!("writing tx hash {:#?}", hex::encode(tx_hash));
                                let db_transaction = DBTransaction {
                                    ops: vec![
                                        Insert {
                                            col: JobsDbColumn::Jobs as u32,
                                            key: DBKey::from_vec(
                                                job_id
                                                    .as_hyphenated()
                                                    .to_string()
                                                    .as_bytes()
                                                    .to_vec(),
                                            ),
                                            value: serde_json::to_vec(&job).unwrap(),
                                        },
                                        Insert {
                                            col: JobsDbColumn::JobsIndex as u32,
                                            key: DBKey::from_slice(&job.index.to_be_bytes()),
                                            value: job_id
                                                .as_hyphenated()
                                                .to_string()
                                                .as_bytes()
                                                .to_vec(),
                                        },
                                    ],
                                };
                                self.jobs.write(db_transaction)?;
                            }
                        }
                    }

                    tracing::debug!(
                        "batch {} - {} new next index:  {}",
                        start_block,
                        batch_end_block,
                        finalized.next_index(),
                    );
                    tracing::debug!("saving last block  to db:  {}", batch_end_block);
                    self.save_last_block(batch_end_block)?;

                    start_block = batch_end_block;
                    batch_end_block = min(batch_end_block + batch_size, finish_block);

                    
                }
                tracing::info!(
                    "relayer sync finish\nlocal:   {} | {},\ncontract:{} | {}",
                    finalized.next_index(),
                    finalized.get_root().to_string(),
                    contract_index,
                    contract_root
                );
            }
        }

        Ok(())
    }

    pub fn save_new_job(&self, job: &Job) -> io::Result<()> {
        let nullifier_key = DBKey::from_slice(&helpers::serialize(job.nullifier).unwrap());

        self.jobs.write(DBTransaction {
            ops: vec![
                /*
                Saving Job info with transaction request to be later retrieved by client
                In case of rollback an existing row is mutated
                TODO: use Borsh instead of JSON
                */
                Insert {
                    col: JobsDbColumn::Jobs as u32,
                    key: DBKey::from_slice(job.id.as_bytes()),
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
        })
    }

    pub fn get_jobs(&self, offset: u64, limit: u64) -> Result<Vec<Job>, String> {
        let mut jobs = vec![];
        let mut index = offset;
        for _ in 0..limit {
            let job_id = self
                .jobs
                .get(JobsDbColumn::JobsIndex as u32, &index.to_be_bytes());
            if let Ok(Some(job_id)) = job_id {
                let job = self.jobs.get(JobsDbColumn::Jobs as u32, &job_id);
                if let Ok(Some(job)) = job {
                    let job: Job = serde_json::from_slice(&job).unwrap();
                    if job.transaction.is_none() {
                        break;
                    }
                    jobs.push(job);
                    index += (OUT + 1) as u64;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(jobs)
    }


    fn get_from_block(&self) -> U64 {
        self
            .jobs
            .get(
                JobsDbColumn::SyncedBlockIndex as u32,
                "last_block".as_bytes(),
            )
            .ok()
            .flatten()
            .map(|from_block| U64::from_big_endian(&from_block))
            .unwrap_or(
                self.settings
                    .web3
                    .start_block
                    .map(|block_num| U64::from(block_num))
                    .unwrap_or(U64::from(0)),
            )
    }

    fn save_last_block(&self, to_block: u64) -> Result<(), SyncError> {
        self.jobs
            .write({
                let mut tx = self.jobs.transaction();
                tx.put(JobsDbColumn::SyncedBlockIndex as u32, "last_block".as_bytes(), &to_block.to_be_bytes());
                tx
            })
            .map_err(|err| {
                SyncError::GeneralError(format!("failed to save last synced block: {:?}", err))
            })
    }
}
