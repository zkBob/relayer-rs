use relayer_rs::{
    configuration::get_config,
    contracts::Pool,
    custody::{
        account::{data_file_path, DataType},
        service::CustodyService,
    },
    startup::Application,
    telemetry::setup_telemetry,
    tx_checker, tx_sender,
    types::job::Job,
};
use std::sync::Mutex;
use tracing::Instrument;

use libzeropool::POOL_PARAMS;
use libzkbob_rs::merkle::MerkleTree;
use tokio::sync::mpsc;

use actix_web::web::Data;

use kvdb_rocksdb::{Database, DatabaseConfig};

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {
    let configuration = get_config().expect("failed to get configuration");

    setup_telemetry(&configuration);

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

    // let custody:Data<RwLock<CustodyService<D>>> = Data::new(RwLock::new(CustodyService::new(
    //     // tx_params,
    //     configuration.custody,
    //     custody_db.clone(),
    // )));

    let custody_db = Data::new(kvdb_rocksdb::Database::open(
        &DatabaseConfig {
            columns: 1, //TBD
            ..Default::default()
        },
        "custody_db/custody",
    )
    .unwrap());

    let custody = CustodyService::<Database>::new_native(
        configuration.custody.clone(),
        custody_db.clone(),
    );

    let app = Application::build(
        configuration,
        sender.clone(),
        pending.clone(),
        finalized.clone(),
        jobs.clone(),
        None,
        custody,
        custody_db,
    )
    .await?;

    tx_sender::start(&app.state, receiver, tree_params, pool);
    tx_checker::start(&app.state);

    app.state
        .sync()
        .instrument(tracing::debug_span!("state sync"))
        .await
        .map_err(|e| {
            tracing::error!("sync failed:\n\t{:#?}", e);
            e
        })
        .unwrap();

    app.run_untill_stopped().await
}
