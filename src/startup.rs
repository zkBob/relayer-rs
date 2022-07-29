use crate::{
    configuration::{Settings, Web3Settings},
    contracts::Pool,
    routes::transactions::{query, transact, TransactionRequest},
};

use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use memo_parser::memoparser::parse_calldata;
// use ethereum_jsonrpc::types::BlockNumber;
use kvdb::KeyValueDB;
use kvdb::{DBOp::Insert, DBTransaction};

use kvdb_memorydb::InMemory as MemoryDatabase;
use libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{engines::Bn256, verifier::VK},
    ff_uint::{Num, NumRepr, Uint},
};
use libzkbob_rs::merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use web3::types::{BlockNumber, Transaction as Web3Transaction};
use web3::types::{Bytes, LogWithMeta, H256, U256};

use std::{
    net::TcpListener,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::sync::mpsc::Sender;

pub type DB<D> = web::Data<Mutex<MerkleTree<D, PoolBN256>>>;

#[derive(Debug, Serialize, Deserialize)]
pub enum JobStatus {
    Created,
    Proving,
    Mining,
    Done,
    Rejected,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub created: SystemTime,
    pub status: JobStatus,
    pub transaction_request: Option<TransactionRequest>,
    pub transaction: Option<Web3Transaction>,
}

pub struct State<D: 'static + KeyValueDB> {
    pub web3: Data<Web3Settings>,
    pub pending: DB<D>,
    pub jobs: Data<D>,
    finalized: DB<D>,
    pub vk: VK<Bn256>,
    pub pool: Pool,
    pub sender: Data<Sender<Data<Job>>>, // rx: Receiver<Transaction>,
}

impl<D: 'static + KeyValueDB> State<D> {
    pub async fn sync(&self) -> Result<(), SyncError> {
        let mut db = self.finalized.lock().expect("failed to acquire lock");

        {
            let pool = &self.pool;
            let (contract_index, contract_root) = pool.root().await?;
            let local_root = db.get_root();
            let local_index = db.next_index();
            tracing::debug!("local root {}", local_root.to_string());
            tracing::debug!("contract root {}", contract_root.to_string());

            if !local_root.eq(&contract_root) {
                let missing_indices: Vec<u64> = (local_index..contract_index.as_u64())
                    .step_by(128) // TODO: use const
                    .into_iter()
                    .map(|i| local_index + (i + 1) * (OUT as u64 + 1))
                    .collect();
                tracing::debug!("mising indices: {:?}", missing_indices);

                for event in get_events(
                    Some(BlockNumber::Earliest),
                    Some(BlockNumber::Latest),
                    None,
                    pool,
                )
                .await
                .unwrap()
                .iter()
                {
                    let index = event.event_data.0 - 128;
                    if let Some(tx_hash) = event.transaction_hash {
                        if let Some(tx) = pool.get_transaction(tx_hash).await.unwrap() {
                            let calldata = &tx.input.0;
                            let calldata = parse_calldata(hex::encode(calldata), None)
                                .expect("Calldata is invalid!");

                            let commit = Num::from_uint_reduced(NumRepr(Uint::from_big_endian(
                                &calldata.out_commit,
                            )));
                            tracing::debug!("index: {}, commit {}", index, commit.to_string());
                            db.add_leafs_and_commitments(vec![], vec![(index.as_u64(), commit)]);
                            tracing::debug!("local root {:#?}", db.get_root().to_string());

                            use kvdb::DBKey;

                            let job = Job {
                                created: SystemTime::now(),
                                status: JobStatus::Done,
                                transaction_request: None,
                                transaction: Some(tx),
                            };
                            let db_transaction = DBTransaction {
                                ops: vec![Insert {
                                    col: 0,
                                    key: DBKey::from_vec(vec![1]),
                                    value: serde_json::to_vec(&job).unwrap(),
                                }],
                            };
                            self.jobs.write(db_transaction)?;

                            //TODO: state.addTx(index, Buffer.from(commitAndMemo, 'hex'))
                        }
                    }
                }
            }

            tracing::debug!("local root after sync {:#?}", db.get_root().to_string());
        }

        Ok(())
    }
}

pub struct Application<D: 'static + KeyValueDB> {
    // web3: Web3Settings,
    server: Server,
    host: String,
    port: u16,
    pub state: Data<State<D>>,
    // host: String,

    // pending: DB<D>,
    // finalized: DB<D>,
    // vk: VK<Bn256>,
    // pub pool: Pool, // rx: Receiver<Transaction>,
}
use libzeropool::{constants::OUT, native::params::PoolBN256};

#[derive(Debug)]
pub enum SyncError {
    BadAbi(std::io::Error),
    GeneralError(String),
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
        sender: Sender<Data<Job>>,
        pending: DB<D>,
        finalized: DB<D>,
        jobs: Data<D>, //We don't realy need a mutex, since all jobs/tx are processed independently
        vk: Option<VK<Bn256>>,
    ) -> Result<Self, std::io::Error> {
        tracing::info!("using config {:#?}", configuration);
        let vk = vk.unwrap_or(configuration.application.get_tx_vk().unwrap());
        let host = configuration.application.host;
        let address = format!("{}:{}", host, configuration.application.port);
        let web3 = Data::new(configuration.web3);

        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();

        let pool = Pool::new(web3.clone()).expect("failed to instantiate pool contract");

        let state = Data::new(State {
            pending,
            finalized,
            vk,
            pool,
            jobs,
            sender: Data::new(sender),
            web3,
        });

        let server = run(listener, state.clone())?;

        Ok(Self {
            server,
            host,
            port,
            state,
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

pub async fn get_events(
    from_block: Option<BlockNumber>,
    to_block: Option<BlockNumber>,
    block_hash: Option<H256>,
    pool: &Pool,
) -> Result<Events, SyncError> {
    let result = pool
        .contract
        .events("Message", from_block, to_block, block_hash, (), (), ());

    let events: Events = result.await?;

    Ok(events)
}

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/tx", web::get().to(query))
            .route("/transact", web::post().to(transact::<D>))
            .app_data(state.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
