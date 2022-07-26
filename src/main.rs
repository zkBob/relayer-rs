use relayer_rs::{
    configuration::get_config,
    routes::transactions::TxRequest,
    startup::Application,
    telemetry::{get_subscriber, init_subscriber},
};



use libzeropool::POOL_PARAMS;
use libzeropool_rs::merkle::MerkleTree;
use tokio::sync::mpsc;

use actix_web::web::Data;
use std::sync::Mutex;

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {
    init_subscriber(get_subscriber(
        "relayer".into(),
        "trace".into(),
        std::io::stdout,
    ));

    let configuration = get_config().expect("failed to get configuration");

    let (sender, mut rx) = mpsc::channel::<TxRequest>(1000);

    let pending =
        MerkleTree::new_native(&Default::default(), "pending.db", POOL_PARAMS.clone()).unwrap();

    let finalized =
        MerkleTree::new_native(&Default::default(), "finalized.db", POOL_PARAMS.clone()).unwrap();

    let pending = Data::new(Mutex::new(pending));
    let finalized = Data::new(Mutex::new(finalized));
    
    let app = Application::build(
        configuration,
        sender.clone(),
        pending.clone(),
        finalized.clone(),
    )
    .await?;

    app.state.sync().await.expect("failed to sync");

    tokio::spawn(async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            let mut p = pending.lock().unwrap();
            p.append_hash(tx.proof.inputs[2], false);
            // tracing::info!("Merkle root {:#?}", p.get_root());
        }
    });

    app.run_untill_stopped().await

}
