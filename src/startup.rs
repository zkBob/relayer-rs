use crate::{
    configuration::{Settings, Web3Settings},
    contracts::Pool,
    routes::transactions::{query, transact, Transaction, TxRequest},
};

use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use memo_parser::memo;
// use ethereum_jsonrpc::types::BlockNumber;
use kvdb::KeyValueDB;

use kvdb_memorydb::InMemory as MemoryDatabase;
use libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{
        engines::Bn256,
        verifier::{self, VK},
    },
    ff_uint::Num,
};
use libzeropool_rs::merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use web3::types::{Bytes, LogWithMeta, H256, U256};
use web3::{futures::future::try_join_all, types::BlockNumber};

use std::{
    collections::HashMap,
    convert::identity,
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
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
    pub transaction: Transaction,
}
pub struct State<D: 'static + KeyValueDB> {
    pub web3: Data<Web3Settings>,
    pub pending: DB<D>,
    pub jobs: Arc<MemoryDatabase>,
    finalized: DB<D>,
    pub vk: VK<Bn256>,
    pub pool: Pool,
    pub sender: Data<Sender<TxRequest>>, // rx: Receiver<Transaction>,
}
impl<D: 'static + KeyValueDB> State<D> {
    pub async fn sync(&self) -> Result<(), SyncError> {
        let db = self.finalized.lock().expect("failed to acquire lock");

        {
            let pool = &self.pool;
            let (contract_index, contract_root) = pool.root().await?;
            let local_root = db.get_root();
            let local_index = db.next_index();
            tracing::debug!("local root {:#?}", local_root);
            tracing::debug!("contract root {:#?}", contract_root);

            let mut finalized = self.finalized.lock().unwrap();
            {
                if !local_root.eq(&contract_root) {
                    let missing_indices: Vec<u64> = (local_index..contract_index.as_u64())
                        .into_iter()
                        .map(|i| local_index + (i + 1) * (OUT as u64 + 1))
                        .collect();
                    tracing::debug!("mising indices: {:?}", missing_indices);

                    // let events = try_join_all(
                    //     get_events(
                    //         Some(BlockNumber::Earliest),
                    //         Some(BlockNumber::Latest),
                    //         None,
                    //         pool,
                    //     )
                    //     .await?
                    //     .iter()
                    //     .map(|event| event.transaction_hash)
                    //     .filter_map(identity)
                    //     .map(|tx_hash| pool.get_transaction(tx_hash)),
                    // )
                    // .await;

                    // let txs = events.unwrap().into_iter().filter_map(identity);
                    // .collect::<Vec<_>>();

                    // for t in txs {
                    //     memo::Memo::parse_memoblock(t.input, txtype)
                    // }

                    let markup: HashMap<CallDataField, (usize, usize)> = [
                        (CallDataField::Selector, (0, 4)),
                        (CallDataField::Nullifier, (4, 32)),
                        (CallDataField::OutCommit, (36, 32)),
                        (CallDataField::TxType, (640, 2)),
                        (CallDataField::MemoSize, (642, 2)),
                        (CallDataField::Memo, (644, 0)),
                    ]
                    .iter()
                    .cloned()
                    .collect();

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
                        let index = event
                            .event_data
                            .0
                            .checked_sub(U256::from(127 as i16))
                            .unwrap(); // const index = Number(returnValues.index) - OUTPLUSONE ???
                        if let Some(tx_hash) = event.transaction_hash {
                            if let Some(tx) = pool.get_transaction(tx_hash).await.unwrap() {
                                let calldata = tx.input.0;

                                let parser = PoolCalldataParser::new();

                                let mut tx_type_bytes: [u8; 4] = [0; 4];
                                let (field_start_pos, field_len) =
                                    markup.get(&CallDataField::TxType).unwrap();
                                tx_type_bytes.copy_from_slice(
                                    &calldata[*field_start_pos..(*field_start_pos + *field_len)],
                                );
                                let tx_type = u32::from_be_bytes(tx_type_bytes);

                                let mut out_commit_bytes: [u8; 16] = [0; 16];
                                let (field_start_pos, field_len) =
                                    markup.get(&CallDataField::OutCommit).unwrap();
                                out_commit_bytes.copy_from_slice(
                                    &calldata[*field_start_pos..(*field_start_pos + *field_len)],
                                );
                                let out_commit = u128::from_be_bytes(out_commit_bytes);

                                let mut memo_size_bytes: [u8; 8] = [0; 8];
                                let (field_start_pos, field_len) =
                                    markup.get(&CallDataField::MemoSize).unwrap();
                                memo_size_bytes.copy_from_slice(
                                    &calldata[*field_start_pos..(*field_start_pos + *field_len)],
                                );
                                let memo_size = usize::from_be_bytes(memo_size_bytes);

                                let (field_start_pos, _) =
                                    markup.get(&CallDataField::Memo).unwrap();

                                let tx_type_shift: usize = match memo::TxType::from_u32(tx_type) {
                                    memo::TxType::Deposit | memo::TxType::Transfer => 16,
                                    memo::TxType::Withdrawal | memo::TxType::DepositPermittable => {
                                        72
                                    }
                                };

                                let memo = &calldata[*field_start_pos + tx_type_shift
                                    ..(*field_start_pos + memo_size)];

                                finalized.add_hash(
                                    u64::try_from(
                                        index
                                            .checked_div(U256::from_dec_str("128").unwrap())
                                            .unwrap(),
                                    )
                                    .unwrap(),
                                    Num::try_from(out_commit).unwrap(), // TODO: deserialize out_commit
                                    false,
                                )

                                //TODO: state.addTx(index, Buffer.from(commitAndMemo, 'hex'))
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub enum CallDataField {
    Selector,
    Nullifier,
    OutCommit,
    TxType,
    MemoSize,
    Memo,
}
struct PoolCalldataParser {
    // pub calldata: Bytes,
    markup: HashMap<CallDataField, (usize, usize)>,
}

static SELECTOR_POS: (usize, usize) = (0, 4);
impl PoolCalldataParser {
    fn new() -> Self {
        let markup: HashMap<CallDataField, (usize, usize)> = [
            (CallDataField::Selector, SELECTOR_POS),
            (CallDataField::Nullifier, (4, 32)),
            (CallDataField::OutCommit, (36, 32)),
            (CallDataField::TxType, (640, 2)),
            (CallDataField::MemoSize, (642, 2)),
            (CallDataField::Memo, (644, 0)),
        ]
        .iter()
        .cloned()
        .collect();

        Self { markup }
    }
    // fn get_field(&self, field_name: CallDataField) -> Result<Vec<u8>, SyncError> {

    // match self.markup.get(&field_name)   {
    //     Some((start, len)) => {

    //         let result = [0;len]; // Doesn't compile
    //         Ok(vec![])

    //     },
    //     None => Err( SyncError::GeneralError("Bad calldata ".to_string()))
    // }

    // }
    //     fn parse(&self, calldata: Vec<u8>) -> (&[u8], U256) {
    //         tracing::info!("got calldata {:#?}", calldata);
    //         let markup = &self.markup;
    //         let mut tx_type_bytes: [u8; 4] = [0; 4];
    //         let (field_start_pos, field_len) = markup.get(&CallDataField::TxType).unwrap();
    //         tx_type_bytes.copy_from_slice(&calldata[*field_start_pos..(*field_start_pos + *field_len)]);
    //         let tx_type = u32::from_be_bytes(tx_type_bytes);

    //         let mut out_commit_bytes: [u8; 32] = [0; 32];
    //         let (field_start_pos, field_len) = markup.get(&CallDataField::OutCommit).unwrap();
    //         let out_commit =
    //             U256::from_big_endian(&calldata[*field_start_pos..(*field_start_pos + *field_len)]);

    //         let mut memo_size_bytes: [u8; 8] = [0; 8];
    //         let (field_start_pos, field_len) = markup.get(&CallDataField::MemoSize).unwrap();
    //         memo_size_bytes
    //             .copy_from_slice(&calldata[*field_start_pos..(*field_start_pos + *field_len)]);
    //         let memo_size = usize::from_be_bytes(memo_size_bytes);

    //         // let memo_bytes = [0;memo_size];

    //         let (field_start_pos, _) = markup.get(&CallDataField::Memo).unwrap();

    //         let memo = &calldata[*field_start_pos..(*field_start_pos + memo_size)];

    //         (memo, out_commit)
    //     }
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
        sender: Sender<TxRequest>,
        pending: DB<D>,
        finalized: DB<D>,
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

        let jobs = Arc::new(kvdb_memorydb::create(2));

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
