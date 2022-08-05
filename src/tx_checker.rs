use actix_web::web::Data;
use std::thread::sleep;
use std::time::{Duration, SystemTime};

use kvdb::{DBKey, DBOp, DBTransaction, KeyValueDB};
use libzeropool::constants::OUTPLUSONELOG;

use crate::helpers::serialize;
use crate::state::{Job, JobStatus, JobsDbColumn, State};

pub fn start_poller<D: KeyValueDB>(state: &Data<State<D>>) -> () {
    let state = state.clone();

    let jobs: Vec<Job> = state
        .jobs
        .iter(JobsDbColumn::TxCheckTasks as u32)
        .map(|(_k, v)| serde_json::from_slice(&v[..]).unwrap())
        .collect();
    tokio::spawn(async move {
        // let pool = Data::new(state.pool);
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

pub async fn check_tx<D: KeyValueDB>(
    mut job: Job,
    // pool: Data<Pool>,
    state: Data<State<D>>,
) -> Result<JobStatus, std::io::Error> {

    let jobs = &state.jobs;
    let tx_hash = job.transaction.as_ref().unwrap().hash;
    tracing::info!("check for tx {} started", tx_hash);
    if let Some(tx_receipt) = state
        .pool
        .get_transaction_receipt(tx_hash)
        .await
        .expect("failed to get transaction receipt")
    {
        if !tx_receipt.status.unwrap().is_zero() {
            tracing::debug!(
                "tx was successfully mined at {}",
                tx_receipt.block_number.unwrap()
            );

            let tx_request = job.transaction_request.as_ref().unwrap();

            // let tx = job.transaction.as_ref().unwrap();

            let mut finalized = state.finalized.lock().unwrap();
            {
                finalized.add_hash_at_height(
                    OUTPLUSONELOG as u32,
                    job.index,
                    job.commitment,
                    false,
                );
            }

            job.status = JobStatus::Done;

            jobs.transaction().put(
                JobsDbColumn::Jobs as u32,
                &DBKey::from_slice(tx_request.uuid.as_ref().unwrap().as_bytes()),
                &serde_json::to_vec(&job).unwrap(),
            );
            return Ok(JobStatus::Done);
        } else {
            tracing::debug!("tx was reverted at {}", tx_receipt.block_number.unwrap());

            let mut pending = state.pending.lock().unwrap();

            let finalized = state.finalized.lock().unwrap();

            pending.rollback(finalized.next_index());

            // For every OTHER transaction waiting to be mined
            for (request_id, _val) in jobs.iter(JobsDbColumn::TxCheckTasks as u32) {
                // Retrieve and deserialize Job information
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

                    jobs.write(transaction)?;
                }
            }
        }
        return Ok(JobStatus::Rejected);
    }
    return Ok(JobStatus::Mining);
}
