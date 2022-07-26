use crate::{
    configuration::{Settings, Web3Settings},
    contracts::Pool,
    routes::transactions::{query, transact, TxRequest},
};

use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};

// use ethereum_jsonrpc::types::BlockNumber;
use kvdb::KeyValueDB;
use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, verifier};
use libzeropool_rs::merkle::MerkleTree;
use web3::types::BlockNumber;
use web3::types::{Bytes, LogWithMeta, H256, U256};

use std::{net::TcpListener, sync::Mutex};
use tokio::sync::mpsc::Sender;

pub type DB<D> = web::Data<Mutex<MerkleTree<D, PoolBN256>>>;
pub struct Application {
    web3: Web3Settings,
    server: Server,
    host: String,
    port: u16,
    // pending: DB<D>,
    // finalized: DB<D>, // rx: Receiver<Transaction>,
}
use libzeropool::{constants::OUT, native::params::PoolBN256};

#[derive(Debug)]
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

impl Application {
    pub async fn build<D: 'static + KeyValueDB>(
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
        let pending_clone = pending.clone();
        let finalized_clone = finalized.clone();
        let server = run::<D>(listener, sender, tx_vk, pending, finalized)?;

        //TODO: should only sync once , then copy received events
        let past_events = tracing::info!("sync complete");
        Ok(Self {
            web3: configuration.web3,
            server,
            host,
            port,
            // pending: pending_clone,
            // finalized: finalized_clone,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_untill_stopped(self) -> Result<(), std::io::Error> {
        tracing::info!("starting webserver at http://{}:{}", self.host, self.port);
        self.server.await
    }
}

type MessageEvent = (U256, H256, Bytes);
type Events = Vec<LogWithMeta<MessageEvent>>;

pub async fn sync_state<D: 'static + KeyValueDB>(
    finalized: DB<D>,
    web3_settings: Web3Settings,
) -> Result<(), SyncError> {
    // let finalized = finalized.lock().expect("failed to acquire lock");
    let events = get_events(
        finalized,
        Data::new(web3_settings),
        Some(BlockNumber::Earliest),
        Some(BlockNumber::Latest),
        None,
    )
    .await?;

    Ok(())
}

pub async fn get_events<D: 'static + KeyValueDB>(
    db: DB<D>,
    web3_settings: Data<Web3Settings>,
    from_block: Option<BlockNumber>,
    to_block: Option<BlockNumber>,
    block_hash: Option<H256>,
) -> Result<Events, SyncError> {
    let pool = Pool::new(web3_settings)?;

    let (contract_index, contract_root) = pool.root().await?;

    tracing::info!("contract root {:#?}", contract_root);

    let db = db.lock().expect("failed to acquire lock");

    {
        let local_root = db.get_root();
        let local_index = db.next_index();
        tracing::debug!("local root {:#?}", local_root);
        tracing::debug!("contract root {:#?}", contract_root);

        if !local_root.eq(&contract_root) {
            let missing_indices: Vec<u64> = (local_index..contract_index.as_u64())
                .into_iter()
                .map(|i| local_index + (i + 1) * (OUT as u64 + 1))
                .collect();
            tracing::debug!("mising indices: {:?}", missing_indices);

            //event Message(uint256 indexed index, bytes32 indexed hash, bytes message);

            let result =
                pool.contract
                    .events("Message", from_block, to_block, block_hash, (), (), ()); //TODO: hide this under the hood?

            let events: Events = result.await?;

            // tracing::debug!("{:?}", events);

            for event in events.iter() {
                if let Some(tx_hash) = event.transaction_hash {

                    if let Some(tx) = pool.get_transaction(tx_hash).await.unwrap() {
                    tracing::info!("got tx {:#?}", tx.input);
                
                    }
                }
            }

            return Ok(events);
        }


    }

    Ok(vec![])
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
