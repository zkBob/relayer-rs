use actix_web::web::Data;
use kvdb::KeyValueDB;
use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters};
use tokio::{sync::mpsc::Receiver, task};
use web3::types::Transaction;

use crate::{
    contracts::Pool,
    state::{Job, JobStatus, JobsDbColumn, State},
    tx,
};

pub fn start<D: KeyValueDB>(
    state: &Data<State<D>>,
    mut receiver: Receiver<Job>,
    tree_params: Parameters<Bn256>,
    pool: Pool,
) -> () {
    let state = state.clone();
    task::spawn(async move {
        tracing::info!("starting tx_sender...");

        while let Some(mut job) = receiver.recv().await {
            let tx_data = {
                let mut pending = state.pending.lock().unwrap();
                let tx_data = tx::build(&job, &pending, &tree_params);

                let transaction_request = job.transaction_request.as_ref().unwrap();
                pending.append_hash(transaction_request.proof.inputs[2], false);

                tx_data
            };

            tracing::info!(
                "[Job: {}] Sending tx with data: {}",
                job.id,
                hex::encode(&tx_data)
            );

            match pool.send_tx(tx_data).await {
                Ok(tx_hash) => {
                    tracing::info!("[Job: {}] Received tx hash: {:#x}", job.id, tx_hash);

                    job.status = JobStatus::Mining;
                    job.transaction = Some(Transaction {
                        hash: tx_hash,
                        ..Default::default()
                    });

                    state
                        .jobs
                        .write({
                            let mut tx = state.jobs.transaction();
                            tx.put(
                                JobsDbColumn::Jobs as u32,
                                job.id.as_bytes(),
                                &serde_json::to_vec(&job).unwrap(),
                            );
                            tx.put(
                                JobsDbColumn::TxCheckTasks as u32,
                                job.id.as_bytes(),
                                &serde_json::to_vec(&job).unwrap(),
                            );
                            tx
                        })
                        .expect("failed to update job status");
                }
                Err(err) => {
                    tracing::warn!("[Job: {}] Failed to send tx with error: {}", job.id, err);
                    
                    job.status = JobStatus::Rejected;

                    state
                        .jobs
                        .write({
                            let mut tx = state.jobs.transaction();
                            tx.put(
                                JobsDbColumn::Jobs as u32,
                                job.id.as_bytes(),
                                &serde_json::to_vec(&job).unwrap(),
                            );
                            tx
                        })
                        .expect("failed to update job status");

                    // TODO: is it okay?
                    state
                        .pending
                        .lock()
                        .unwrap()
                        .rollback(state.finalized.lock().unwrap().next_index());
                }
            }
        }
    });
}
