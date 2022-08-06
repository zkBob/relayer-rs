use relayer_rs::{
    configuration::get_config,
    startup::Application,
    telemetry::{get_subscriber, init_subscriber}, state::Job, tx, contracts::Pool,
};

use libzeropool::POOL_PARAMS;
use libzkbob_rs::merkle::MerkleTree;
use tokio::sync::mpsc;

use actix_web::web::Data;
use std::sync::Mutex;

use kvdb_rocksdb::{Database, DatabaseConfig};

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {
    init_subscriber(get_subscriber(
        "relayer".into(),
        "trace".into(),
        std::io::stdout,
    ));

    let configuration = get_config().expect("failed to get configuration");

    let (sender, mut rx) = mpsc::channel::<Data<Job>>(1000);

    let pending =
        MerkleTree::new_native(Default::default(), "pending.db", POOL_PARAMS.clone()).unwrap();

    let finalized =
        MerkleTree::new_native(Default::default(), "finalized.db", POOL_PARAMS.clone()).unwrap();

    let jobs = Data::new(Database::open(
        &DatabaseConfig {
            columns: 2,
            ..Default::default()
        },
        "jobs.db",
    )?);

    let pending = Data::new(Mutex::new(pending));
    let finalized = Data::new(Mutex::new(finalized));
    
    let tree_params = configuration.application.get_tree_params();
    let pool = Pool::new(Data::new(configuration.web3.clone())).expect("failed to instantiate pool contract");

    let app = Application::build(
        configuration,
        sender.clone(),
        pending.clone(),
        finalized.clone(),
        jobs,
        None,
    )
    .await?;

    app.state.sync().await.expect("failed to sync");

    
    tokio::spawn(async move {
        tracing::info!("starting receiver");
        
        while let Some(job) = rx.recv().await {
            let tx_data = {
                let mut p = pending.lock().unwrap();
                let tx_data = tx::build(&job, &p, &tree_params);
                
                let transaction_request = job.transaction_request.as_ref().unwrap();
                p.append_hash(transaction_request.proof.inputs[2], false);
                
                tx_data
            };
            pool.send_tx(tx_data).await;
            
        }
    });

    app.run_untill_stopped().await
}
