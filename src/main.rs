use std::sync::Mutex;

use opentelemetry::trace::Tracer;
use opentelemetry::{sdk::trace::Tracer as SDK_TRACER, global};
use opentelemetry_jaeger::new_agent_pipeline;
use opentelemetry::sdk::export::trace::stdout;
use relayer_rs::{
    configuration::get_config,
    contracts::Pool,
    startup::Application,
    state::Job,
    telemetry::init_jaeger,
    tx_checker, tx_sender,
};
use opentelemetry::runtime::Tokio;

use libzeropool::POOL_PARAMS;
use libzkbob_rs::merkle::MerkleTree;
use tokio::sync::mpsc;

use actix_web::web::Data;

use kvdb_rocksdb::{Database, DatabaseConfig};

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {

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
    let pool = Pool::new(Data::new(configuration.web3.clone()))
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

    

    tx_sender::start(&app.state, receiver, tree_params, pool);
    tx_checker::start(&app.state);

    

        // app.state
        // .sync()
        // .await
        // .map_err(|e| {
        //     tracing::error!("sync failed:\n\t{:#?}", e);
        //     e
        // })
        // .unwrap();


    
    app.run_untill_stopped().await
}
