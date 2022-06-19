use std::{fs::File, io::BufReader};

use web3::{
    contract::{Contract, Options},
    transports::Http,
    types::{H160, U256},
};

use crate::{configuration:: Web3Settings, startup::SyncError};

pub struct Pool {
    pub contract: Contract<Http>,
}

impl Pool {
    pub fn new(config: &Web3Settings) -> Result<Self, SyncError> {
        let http = web3::transports::Http::new(&config.provider_endpoint)
            .expect("failed to init web3 provider");

        let web3 = web3::Web3::new(http);
        let file = BufReader::new(File::open(&config.abi_path)?);

        let contract = Contract::from_json(
            web3.eth(),
            H160::from_slice(config.pool_address.as_bytes()),
            file.buffer(),
        )
        .expect("failed to init pool contract interface");

        Ok(Self{contract})
    }
    pub async fn root(&self) -> Result<(U256, String), SyncError> {
        let contract = &self.contract;
        let result = contract.query("pool_index", (), None, Options::default(), None);
        let pool_index: U256 = result.await?;

        let result = contract.query("roots", (pool_index,), None, Options::default(), None);

        let root: String = result.await?;

        tracing::debug!("got root from contract {}", root);

        Ok((pool_index, root))
    }
}
