use std::sync::Mutex;

use actix_web::web::Data;
use kvdb_memorydb::InMemory;
use libzeropool::fawkes_crypto::backend::bellman_groth16::engines::Bn256;
use libzeropool::native::params::PoolBN256;
use libzeropool::POOL_PARAMS;
use libzkbob_rs::merkle::MerkleTree;
use once_cell::sync::Lazy;
use relayer_rs::configuration::{get_config, Settings};
use relayer_rs::contracts::Pool;
use relayer_rs::routes::transactions::TxRequest;
use relayer_rs::startup::Application;
use relayer_rs::telemetry::{get_subscriber, init_subscriber};
use tokio::sync::mpsc;

use crate::generator::Generator;
pub struct TestApp {
    pub config: Settings,
    pub address: String,
    pub port: u16,
    pending: DB,
    pub finalized: DB,
    pub generator: Option<Generator>, // pub pool: Pool
}
type DB = Data<Mutex<MerkleTree<InMemory, PoolBN256>>>;

static TRACING: Lazy<()> = Lazy::new(|| {
    if std::env::var("TEST_LOG").is_ok() {
        init_subscriber(get_subscriber(
            "test".into(),
            "info".into(),
            std::io::stdout,
        ))
    } else {
        init_subscriber(get_subscriber("test".into(), "info".into(), std::io::sink))
    }
});

pub async fn spawn_app(setup: bool) -> Result<TestApp, std::io::Error> {
    Lazy::force(&TRACING);
    let config: Settings = {
        let mut c = get_config().expect("failed to get config");
        c.application.port = 0;
        c
    };

    let mut generator: Option<Generator> = None;

    if setup {
        tracing::info!("Generating cicuit params for the app, don't forget to add --release, otjerwise it would be a loooooong wait");
        generator = Some(Generator::new(
            "6cbed15c793ce57650b9877cf6fa156fbef513c4e6134f022a85b1ffdd59b2a1", //TODO: move to config
        ));
    } else {
        tracing::info!("Using pre-built cicuit params specified in the configuration file");
    }

    let (sender, mut rx) = mpsc::channel::<TxRequest>(1000);

    let pending = Data::new(Mutex::new(MerkleTree::new_test(POOL_PARAMS.clone())));

    let finalized = Data::new(Mutex::new(MerkleTree::new_test(POOL_PARAMS.clone())));

    let pending_clone = pending.clone();

    let finalized_clone = finalized.clone();

    let app = Application::build(
        config.clone(),
        sender,
        pending,
        finalized,
        generator.as_ref().map(|g| g.tx_params.get_vk()),
    )
    .await?;

    app.state.sync().await.expect("failed to sync state");

    let port = app.port();

    let address = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        tracing::info!("starting Receiver for Jobs channel");
        while let Some(job) = rx.recv().await {
            tracing::info!("Received tx {:#?}", job.transaction.memo);
        }
    });

    tokio::spawn(app.run_untill_stopped());

    Ok(TestApp {
        config,
        address,
        port,
        pending: pending_clone,
        finalized: finalized_clone,
        generator,
    })
}
