use std::sync::mpsc::Sender;

use actix_web::web::Data;

use kvdb::KeyValueDB;
use web3::types::TransactionReceipt;

use crate::{
    contracts::Pool,
    state::{Job, State},
};

async fn check_tx<D: KeyValueDB>(
    job: Data<Job>,
    sender: Data<Sender<Data<Job>>>,
    pool: Data<Pool>,
    state: Data<State<D>>,
) {
    let tx_hash = job.transaction.as_ref().unwrap().hash;
    if let Some(tx_receipt) = pool.get_transaction_receipt(tx_hash).await.unwrap() {
        if !tx_receipt.status.unwrap().is_zero() {
            tracing::debug!(
                "tx was successfully mined at {}",
                tx_receipt.block_number.unwrap()
            );

            let tx_request = job.transaction_request.as_ref().unwrap();

            let tx = job.transaction.as_ref().unwrap();

            let mut finalized = state.finalized.lock().unwrap();
            {
                finalized.add_hash(job.index.try_into().unwrap(), job.commitment, false);
            }
        }
    }
}
