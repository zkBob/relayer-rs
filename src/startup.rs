use crate::{
    configuration::{Settings, Web3Settings},
    contracts::Pool,
    routes::transactions::{query, transact, TxRequest},
};

use actix_web::{dev::Server, middleware, web, App, HttpServer};
use kvdb::KeyValueDB;
use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, verifier};
use libzeropool_rs::merkle::MerkleTree;

use std::{net::TcpListener, sync::Mutex};
use tokio::sync::mpsc::Sender;

pub type DB<D> = web::Data<Mutex<MerkleTree<D, PoolBN256>>>;
pub struct Application<D: KeyValueDB> {
    web3: Web3Settings,
    server: Server,
    host: String,
    port: u16,
    pending: DB<D>,
    finalized: DB<D>, // rx: Receiver<Transaction>,
}
use libzeropool::{constants::OUT, native::params::PoolBN256};

pub enum SyncError {
    BadAbi(std::io::Error),

    ContractException(web3::contract::Error),
}
impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::BadAbi(e)
    }
}
impl From<web3::contract::Error> for SyncError {
    fn from(e: web3::contract::Error) -> Self {
        SyncError::ContractException(e)
    }
}

impl<D: 'static + KeyValueDB> Application<D> {
    pub async fn build(
        configuration: Settings,
        sender: Sender<TxRequest>,
        pending: DB<D>,
        finalized: DB<D>,
    ) -> Result<Self, std::io::Error> {
        tracing::info!("using config {:#?}", configuration);
        let tx_vk = configuration.application.get_tx_vk().unwrap();
        let host = configuration.application.host;
        let address = format!("{}:{}", host, configuration.application.port);

        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();
        let server = run::<D>(listener, sender, tx_vk, pending.clone(), finalized.clone())?;

        Ok(Self {
            web3: configuration.web3,
            server,
            host,
            port,
            pending: pending.clone(),
            finalized: finalized.clone(),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_untill_stopped(self) -> Result<(), std::io::Error> {
        tracing::info!("starting webserver at http://{}:{}", self.host, self.port);
        self.server.await
    }

    async fn sync_state(&self) -> Result<(), SyncError> {
        let pool = Pool::new(&self.web3)?;

        let (contract_index, contract_root) = pool.root().await?;

        let finalized = self.finalized.lock().unwrap();

        let local_root = finalized.get_root();
        let local_index = finalized.next_index();

        if local_root.to_string() != contract_root {
            let missing_indices: Vec<u64> = (local_index..contract_index.as_u64())
                .into_iter()
                .map(|i| local_index + (i + 1) * (OUT as u64 + 1))
                .collect();

            tracing::debug!("mising indices: {:?}", missing_indices);

            let result = pool.contract.events("Message", (), (), ()); //TODO: hide this under the hood

            let events: Vec<Vec<u8>> = result.await?;

            tracing::debug!("{:?}", events);
        }

        Ok(())
    }
}

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    sender: Sender<TxRequest>,
    vk: verifier::VK<Bn256>,
    pending: DB<D>,
    finalized: DB<D>,
) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");
    let sender = web::Data::new(sender);

    let vk = web::Data::new(vk);

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/tx", web::get().to(query))
            .route("/transact", web::post().to(transact))
            .app_data(sender.clone())
            .app_data(vk.clone())
            .app_data(pending.clone())
            .app_data(finalized.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
