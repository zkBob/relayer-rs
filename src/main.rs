use std::sync::Mutex;

use relayer_rs::{
    configuration::get_config,
    contracts::Pool,
    startup::Application,
    telemetry::{get_subscriber, init_subscriber},
    tx_checker, tx_sender, types::job::Job,
};

use libzeropool::POOL_PARAMS;
use libzkbob_rs::merkle::MerkleTree;
use tokio::sync::mpsc;

use actix_web::web::Data;

use kvdb_rocksdb::{Database, DatabaseConfig};

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {
    init_subscriber(get_subscriber(
        "relayer".into(),
        "trace".into(),
        std::io::stdout,
    ));

    let configuration = get_config().expect("failed to get configuration");

    let (sender, receiver) = mpsc::channel::<Job>(1000);

    let pending =
        MerkleTree::new_native(Default::default(), "pending.db", POOL_PARAMS.clone()).unwrap();

    let finalized =
        MerkleTree::new_native(Default::default(), "finalized.db", POOL_PARAMS.clone()).unwrap();

    let jobs = Data::new(Database::open(
        &DatabaseConfig {
            columns: 4,
            ..Default::default()
        },
        "jobs.db",
    )?);

    let pending = Data::new(Mutex::new(pending));
    let finalized = Data::new(Mutex::new(finalized));

    let tree_params = configuration.application.get_tree_params();
    let pool = Pool::new(&Data::new(configuration.web3.clone()))
        .expect("failed to instantiate pool contract");

    let app = Application::build(
        configuration,
        sender.clone(),
        pending.clone(),
        finalized.clone(),
        jobs.clone(),
        None,
    )
    .await?;

    app.state.sync().await.expect("failed to sync");

    tx_sender::start(&app.state, receiver, tree_params, pool);
    tx_checker::start(&app.state);

    app.run_untill_stopped().await
}
