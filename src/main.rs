use relayer_rs::{
    configuration::get_config,
    routes::transactions::TxRequest,
    telemetry::{get_subscriber, init_subscriber}, startup::run,
};
use std::net::TcpListener;

use libzeropool::POOL_PARAMS;
use tokio::sync::mpsc;
use libzeropool_rs::merkle::MerkleTree;

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {
    init_subscriber(get_subscriber(
        "relayer".into(),
        "trace".into(),
        std::io::stdout,
    ));

    let configuration = get_config().expect("failed to get configuration");
    let address = format!(
        "{}:{}",
        configuration.application.host, configuration.application.port
    );
    let listener = TcpListener::bind(address)?;

    let vk = configuration.application.get_tx_vk()?;

    let (sender, mut rx) = mpsc::channel::<TxRequest>(1000);

    let mut tree = MerkleTree::new_test(POOL_PARAMS.clone());
    tokio::spawn( async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            tree.append_hash(tx.proof.inputs[2], false);
            tracing::info!("Merkle root {:#?}", tree.get_root());
        }
    });

    run(listener, sender, vk)?.await
}
