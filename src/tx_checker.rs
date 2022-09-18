use actix_web::web::Data;
use std::time::Duration;
use tokio::task;

use kvdb::{DBKey, DBOp, DBTransaction, KeyValueDB};

use crate::helpers::serialize;
use crate::state::{JobsDbColumn, State};
use crate::types::job::{Job, JobStatus};

pub fn start<D: KeyValueDB>(state: &Data<State<D>>) -> () {
    let state = state.clone();
    task::spawn(async move {
        tracing::info!("starting tx_checker...");
        loop {
            let jobs: Vec<Job> = state
                .jobs
                .iter(JobsDbColumn::TxCheckTasks as u32)
                .map(|(_k, v)| serde_json::from_slice(&v[..]).unwrap())
                .collect();

            for job in jobs.into_iter() {
                let state = state.clone();
                let job_id:&str = &job.id.clone();
                match check_tx(job, state).await {
                    Err(_) | Ok(JobStatus::Rejected) => {
                        tracing::warn!(
                            "job {} caught error / revert ",
                            job_id
                        );
                        break;
                    }
                    _ => (),
                }
            }
            tokio::time::sleep(Duration::from_secs(state.web3.scheduler_interval_sec)).await;
        }
    });
}

pub async fn check_tx<D: KeyValueDB>(
    mut job: Job,
    state: Data<State<D>>,
) -> Result<JobStatus, std::io::Error> {
    let jobs = &state.jobs;
    let tx_hash = job.transaction.as_ref().unwrap().hash;
    tracing::info!("check for tx {:#x} started", tx_hash);
    if let Some(tx_receipt) = state
        .pool
        .get_transaction_receipt(tx_hash)
        .await
        .expect("failed to get transaction receipt")
    {
        if !tx_receipt.status.unwrap().is_zero() {
            tracing::debug!(
                "tx {:#x} was successfully mined at {}",
                tx_hash,
                tx_receipt.block_number.unwrap()
            );

            state
                .finalized
                .lock()
                .unwrap()
                .add_leafs_and_commitments(vec![], vec![(job.index, job.commitment)]);

            job.status = JobStatus::Done;

            jobs.write({
                let mut tx = jobs.transaction();
                tx.put(
                    JobsDbColumn::Jobs as u32,
                    job.id.as_bytes(),
                    &serde_json::to_vec(&job).unwrap(),
                );
                tx.delete(JobsDbColumn::TxCheckTasks as u32, job.id.as_bytes());
                tx
            })
            .expect("failed to update job status");

            return Ok(JobStatus::Done);
        } else {
            tracing::debug!(
                "tx {:#x} was reverted at {}",
                tx_hash,
                tx_receipt.block_number.unwrap()
            );

            let mut pending = state.pending.lock().unwrap();

            let finalized = state.finalized.lock().unwrap();

            pending.rollback(finalized.next_index());

            // For every transaction waiting to be mined, including the one that is being processed right now
            for (request_id, _val) in jobs.iter(JobsDbColumn::TxCheckTasks as u32) {
                // Retrieve and deserialize Job information
                tracing::info!(
                    "rejecting task {:?}",
                    String::from_utf8(request_id.to_vec()).unwrap()
                );
                if let Some(other_job) = jobs.get(JobsDbColumn::Jobs as u32, &request_id).unwrap() {
                    let mut job: Job = serde_json::from_slice(&other_job[..])?;

                    // Mark job as rejected
                    job.status = JobStatus::Rejected;

                    // Save to store for the client to find out about it eventualy
                    let reject_job_operation = DBOp::Insert {
                        col: JobsDbColumn::Jobs as u32,
                        key: DBKey::from_slice(&request_id),
                        value: serde_json::to_vec(&job).unwrap(),
                    };

                    let delete_nullifier_operation = DBOp::Delete {
                        col: JobsDbColumn::Nullifiers as u32,
                        key: DBKey::from_slice(&serialize(job.nullifier).unwrap()),
                    };

                    let transaction = DBTransaction {
                        ops: vec![reject_job_operation, delete_nullifier_operation],
                    };

                    jobs.write(transaction)?; //TODO: collect all updates in a single transaction for consistency purpose
                }
            }
        }
        return Ok(JobStatus::Rejected);
    }
    return Ok(JobStatus::Mining);
}
