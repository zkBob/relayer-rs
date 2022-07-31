use std::sync::Mutex;

use actix_web::web::Data;
use kvdb_memorydb::InMemory;

use libzeropool::fawkes_crypto::backend::bellman_groth16::verifier::VK;
use libzeropool::native::params::PoolBN256;
use libzeropool::POOL_PARAMS;
use libzkbob_rs::merkle::MerkleTree;
use once_cell::sync::Lazy;
use relayer_rs::configuration::{get_config, Settings};
use relayer_rs::startup::{Application, Job, State};
use relayer_rs::telemetry::{get_subscriber, init_subscriber};
use tokio::sync::mpsc;

use libzeropool::fawkes_crypto::backend::bellman_groth16::setup;
use libzeropool::{
    circuit::tx::{c_transfer, CTransferPub, CTransferSec},
    fawkes_crypto::{
        backend::bellman_groth16::{engines::Bn256, Parameters},
        engines::bn256::Fr,
    }
};

use crate::generator::Generator;
use libzeropool::fawkes_crypto::circuit::cs::CS;
pub struct TestApp {
    pub config: Settings,
    pub address: String,
    pub port: u16,
    state: Data<State<InMemory>>,
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

pub async fn spawn_app(gen_params: bool) -> Result<TestApp, std::io::Error> {
    Lazy::force(&TRACING);
    let config: Settings = {
        let mut c = get_config().expect("failed to get config");
        c.application.port = 0;
        c
    };

    let mut generator: Option<Generator> = None;

    if gen_params {
        let client_key = config
            .application
            .tx
            .client_mock_key
            .as_ref()
            .expect("a private key for client mock is expected");
        // tracing::info!("Generating cicuit params for the app, don't forget to add --release, otjerwise it would be a loooooong wait");
        let tx_params_path = &config.application.tx.params;

        // let path = format!("{}", tx_params_folder);
        // tracing::info!("trying to load params from {}", path);
        let f = std::fs::read(tx_params_path).unwrap();
        
        let tx_params = {
            match Parameters::<Bn256>::read(&mut f[..].as_ref(), true, true) {
                Ok(params) => params,
                Err(_) => {
                    tracing::debug!("pre-built parameters not found, generating a new set of params");
                fn circuit<C: CS<Fr = Fr>>(public: CTransferPub<C>, secret: CTransferSec<C>) {
                    c_transfer(&public, &secret, &*POOL_PARAMS);
                }
                setup::setup::<Bn256, _, _, _>(circuit)
                },
            }
        };
        // let tx_params = std::fs::read(tx_params_folder)
        //     .map(|f| Parameters::<Bn256>::read(&mut f[..].as_ref(), true, true).unwrap())
        //     .unwrap_or({
        //         tracing::debug!("pre-built parameters not found, generating a new set of params");
        //         fn circuit<C: CS<Fr = Fr>>(public: CTransferPub<C>, secret: CTransferSec<C>) {
        //             c_transfer(&public, &secret, &*POOL_PARAMS);
        //         }
        //         setup::setup::<Bn256, _, _, _>(circuit)
        //     });
        generator = Some(Generator::new(&client_key, Some(tx_params)));
    } else {
        tracing::info!("Using pre-built cicuit params specified in the configuration file");
    }

    let (sender, mut rx) = mpsc::channel::<Data<Job>>(1000);

    let pending = Data::new(Mutex::new(MerkleTree::new_test(POOL_PARAMS.clone())));

    let finalized = Data::new(Mutex::new(MerkleTree::new_test(POOL_PARAMS.clone())));

    let jobs = Data::new(kvdb_memorydb::create(2));

    let vk_str = std::fs::read_to_string(&config.application.tx.vk).unwrap();

    let prebuilt_vk: VK<Bn256> = serde_json::from_str(&vk_str).unwrap();

    let vk = generator.as_ref().map(|g| {
        g.tx_params
            .as_ref()
            .map(|p| p.get_vk())
            .unwrap_or(prebuilt_vk)
    });

    let app = Application::build(config.clone(), sender, pending, finalized, jobs, vk).await?;

    let state = app.state.clone();

    // app.state.sync().await.expect("failed to sync state");

    let port = app.port();

    let address = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        tracing::info!("starting Receiver for Jobs channel");
        while let Some(job) = rx.recv().await {
            if let Some(transaction_request) = job.transaction_request.as_ref() {
                tracing::info!("Received tx {:#?}", transaction_request.memo);
            }
        }
    });

    tokio::spawn(app.run_untill_stopped());

    Ok(TestApp {
        config,
        address,
        port,
        state ,
        generator,
    })
}
