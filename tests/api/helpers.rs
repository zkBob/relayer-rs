use std::sync::Mutex;

use actix_web::web::Data;
use kvdb_memorydb::InMemory;
use libzeropool::native::params::PoolBN256;
use libzeropool::POOL_PARAMS;
use libzeropool_rs::merkle::MerkleTree;
use once_cell::sync::Lazy;
use relayer_rs::configuration::{get_config, Settings};
use relayer_rs::contracts::Pool;
use relayer_rs::routes::transactions::TxRequest;
use relayer_rs::startup::Application;
use relayer_rs::telemetry::{get_subscriber, init_subscriber};
use tokio::sync::mpsc;
pub struct TestApp {
    pub config: Settings,
    pub address: String,
    pub port: u16,
    pending: DB,
    pub finalized: DB,
    pub pool: Pool
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

pub async fn spawn_app() -> Result<TestApp, std::io::Error> {
    Lazy::force(&TRACING);
    let config: Settings = {
        let mut c = get_config().expect("failed to get config");
        c.application.port = 0;
        c
    };

    let (sender, mut rx) = mpsc::channel::<TxRequest>(1000);

    let pending = Data::new(Mutex::new(MerkleTree::new_test(POOL_PARAMS.clone())));

    let finalized = Data::new(Mutex::new(MerkleTree::new_test(POOL_PARAMS.clone())));

    let pending_clone = pending.clone();

    let finalized_clone = finalized.clone();

    let app = Application::build(config.clone(), sender, pending, finalized).await?;

    let port = app.port();

    let address = format!("http://127.0.0.1:{}", port);

    let web3_config = config.web3.clone();

    let pool = Pool::new(Data::new(web3_config)).expect("failed to initialize pool contract");

    tokio::spawn(async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            tracing::info!("Received tx {:#?}", tx.memo);
        }
    });

    tokio::spawn(app.run_untill_stopped());

    Ok(TestApp {
        config,
        address,
        port,
        pending: pending_clone,
        finalized: finalized_clone,
        pool
    })
}
