use std::net::TcpListener;

use libzeropool::POOL_PARAMS;
use relayer_rs::{telemetry::{init_subscriber, get_subscriber}, configuration::get_config, startup::run, routes::transactions::Transaction, merkle::MerkleTree};
use tokio::sync::mpsc;


#[actix_web::main]
async fn main() -> Result<(), std::io::Error>{
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

    let (sender, mut rx) = mpsc::channel::<Transaction>(1000);

    let mut tree = MerkleTree::new_test(POOL_PARAMS.clone());
    tokio::spawn( async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            tree.append_hash(tx.proof.inputs[2], false);
            tracing::info!("Merkle root {:#?}", tree.get_root());
        }
    });

    run(
        listener,
        sender
    )?
    .await
 
}
