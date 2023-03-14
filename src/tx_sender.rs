use actix_web::web::Data;
use kvdb::KeyValueDB;
use libzkbob_rs::libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters};
use tokio::{sync::mpsc::{Receiver, error::TryRecvError}, task};
use web3::types::Transaction;
use tracing_futures::Instrument;
use crate::{
    contracts::Pool,
    state::{JobsDbColumn, State},
    tx, types::job::{Job, JobStatus},
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

        while let Some(job) = receiver.recv().await {
            process_job(job, &state, &tree_params, &pool).await;
        }
    });
}

pub async fn receive_one<D: KeyValueDB>(
    state: &Data<State<D>>,
    mut receiver: Receiver<Job>,
    tree_params: Parameters<Bn256>,
    pool: Pool,
) {

    match  receiver.try_recv()  {
        Ok(job) => {
            let span = tracing::debug_span!("processing new message from channel", id = job.id);
            process_job(job, state, &tree_params, &pool).instrument(span).await
        },
        Err(TryRecvError::Empty) => tracing::error!("WTF NO MESSAGE"),
        Err(error) => tracing::error!("{:#?}" ,error)
    };
        
}

pub async fn process_job<D: KeyValueDB>(
    mut job: Job,
    state: &Data<State<D>>,
    tree_params: &Parameters<Bn256>,
    pool: &Pool,
) {
    let tx_data = {
        let mut pending = state.pending.lock().await;
        let tx_data = tx::build(&job, &pending, tree_params);

        job.index = pending.next_index();
        pending.add_leafs_and_commitments(vec![], vec![(job.index, job.commitment)]);

        tx_data
    };

    tracing::info!(
        "[Job: {}] Sending tx with data: {}",
        job.id,
        hex::encode(&tx_data)
    );

    let span = tracing::debug_span!("sending transaction to contract", id = job.id);
    match pool.send_tx(tx_data).instrument(span).await {
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
                        JobsDbColumn::JobsIndex as u32,
                        &job.index.to_be_bytes(),
                        job.id.as_bytes(),
                    );
                    tx.put(
                        JobsDbColumn::TxCheckTasks as u32,
                        job.id.as_bytes(),
                        &serde_json::to_vec(&job).unwrap(),
                    );
                    tx
                })
                .expect("failed to update job status");
                tracing::info!("[Job: {}] Sent successfully to the contract", job.id);
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
                .await
                .rollback(state.finalized.lock().await.next_index());
        }
    }
}
