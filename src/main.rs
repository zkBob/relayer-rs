use relayer_rs::{
    configuration::{get_config, Settings},
    routes::transactions::TxRequest,
    startup::run,
    telemetry::{get_subscriber, init_subscriber},
};
use web3::{
    contract::{Contract, Options},
    types::{H160, U256}, transports::Http,
};

use std::{fs::File, io::BufReader, net::TcpListener};

use libzeropool::{
    constants::{OUT, OUTPLUSONELOG},
    native::params::{PoolBN256, PoolParams},
    POOL_PARAMS,
};
use libzeropool_rs::merkle::MerkleTree;
use tokio::sync::mpsc;

use kvdb_rocksdb::{Database as NativeDatabase, DatabaseConfig};

use ethabi::{Event};

enum SyncError {
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

fn get_contract_instance(config: Settings) -> Result <Contract<Http>, SyncError> {
    let http = web3::transports::Http::new(&config.web3.provider_endpoint)
        .expect("failed to init web3 provider");

    let web3 = web3::Web3::new(http);
    let file = BufReader::new(File::open(config.web3.abi_path)?);

    let contract = Contract::from_json(
        web3.eth(),
        H160::from_slice(config.web3.pool_address.as_bytes()),
        file.buffer(),
    )
    .expect("failed to init pool contract interface");

    Ok(contract)
}
async fn get_contract_root(contract: &Contract<Http>) -> Result<(U256, String), SyncError> {
    
    
    let result = contract.query("pool_index", (), None, Options::default(), None);
    let pool_index: U256 = result.await?;

    let result = contract.query("roots", (pool_index,), None, Options::default(), None);

    let root: String = result.await?;

    tracing::debug!("got root from contract {}", root);

    Ok((pool_index, root))
}

async fn sync_state(
    mut current_state: MerkleTree<NativeDatabase, PoolBN256>,
    config: Settings,
) -> Result<(), SyncError> {

    let contract = get_contract_instance(config)?;

    let (contract_index, contract_root) = get_contract_root( &contract).await?;

    let local_root = current_state.get_root();
    let local_index = current_state.next_index();

    
    if local_root.to_string() != contract_root {

        let missing_indices:Vec<u64> = (local_index..contract_index.as_u64())
            .into_iter()
            .map(|i| local_index + (i + 1) * (OUT as u64 + 1))
            .collect();


            tracing::debug!("mising indices: {:?}", missing_indices);

            let result  = contract.events("Message", (), (), ());
            
            let events: Vec<Vec<u8>> = result.await?;

    }

    
    Ok(())
}

#[actix_web::main]
async fn main() -> Result<(), std::io::Error> {
    init_subscriber(get_subscriber(
        "relayer".into(),
        "trace".into(),
        std::io::stdout,
    ));

    let configuration = get_config().expect("failed to get configuration");
    let address = format!(
        "{}:{}",
        configuration.application.host, configuration.application.port
    );
    let listener = TcpListener::bind(address)?;

    let vk = configuration.application.get_tx_vk()?;

    let (sender, mut rx) = mpsc::channel::<TxRequest>(1000);

    let mut tree =
        MerkleTree::new_native(&Default::default(), "tx.db", POOL_PARAMS.clone()).unwrap();

    tokio::spawn(async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            tree.append_hash(tx.proof.inputs[2], false);
            tracing::info!("Merkle root {:#?}", tree.get_root());
        }
    });

    run(listener, sender, vk)?.await
}
