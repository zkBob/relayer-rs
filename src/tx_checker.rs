use std::{
    sync::mpsc::Sender,
    time::{Duration, SystemTime},
};

use actix_web::web::Data;
use std::thread::sleep;

use kvdb::{DBKey, DBOp, DBTransaction, KeyValueDB};
use libzeropool::constants::OUTPLUSONELOG;

use crate::{
    contracts::Pool,
    state::{Job, JobStatus, JobsDbColumn, State},
};

fn start_poller<D: KeyValueDB>(state: Data<State<D>>, pool: Data<Pool>) -> () {
    tokio::spawn(async move {
        for (id, job) in state.jobs.iter(JobsDbColumn::TxCheckTasks as u32) {
            let state = state.clone();
            let pool = pool.clone();
            tracing::debug!("polling started at {:?}", SystemTime::now());
            match check_tx(serde_json::from_slice(&job[..]).unwrap(), pool, state).await {
                Err(_) | Ok(JobStatus::Rejected) => {
                    tracing::warn!(
                        "caught error / revert at {}",
                        String::from_utf8(id.to_vec()).unwrap()
                    );
                    break;
                }
                _ => (),
            }
        }
        sleep(Duration::from_secs(5));
    });
}

// async fn poll<D: KeyValueDB>(state: Data<State<D>>, pool: Data<Pool>) {
//     for (id, job) in state.jobs.iter(JobsDbColumn::TxCheckTasks as u32) {
//         let state = state.clone();
//         let pool = pool.clone();
//         tracing::debug!("polling started at {:?}", SystemTime::now());
//         match check_tx(serde_json::from_slice(&job[..]).unwrap(), pool, state).await {
//             Err(_) | Ok(JobStatus::Rejected) => {
//                 tracing::warn!(
//                     "caught error / revert at {}",
//                     String::from_utf8(id.to_vec()).unwrap()
//                 );
//                 break;
//             }
//             _ => (),
//         }
//     }

//     // check_tx(job, sender, pool, state)
// }

async fn check_tx<D: KeyValueDB>(
    mut job: Job,
    pool: Data<Pool>,
    state: Data<State<D>>,
) -> Result<JobStatus, std::io::Error> {
    let jobs = &state.jobs;
    let tx_hash = job.transaction.as_ref().unwrap().hash;
    if let Some(tx_receipt) = pool
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

            let tx = job.transaction.as_ref().unwrap();

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
                        key: DBKey::from_slice(job.nullifier.as_bytes()),
                    };

                    let transaction = DBTransaction {
                        ops: vec![reject_job_operation, delete_nullifier_operation],
                    };

                    let error_message = &format!(
                        "failed to rollback job {:#?}",
                        String::from_utf8(request_id.to_vec()).unwrap()
                    );

                    jobs.write(transaction)?;
                }
            }
        }
        return Ok(JobStatus::Rejected);
    }
    return Ok(JobStatus::Mining);
}
