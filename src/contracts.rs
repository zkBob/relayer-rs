use std::str::FromStr;

use libzeropool::fawkes_crypto::{ff_uint::Num, engines::bn256::Fr};
use web3::{
    contract::{Contract, Options},
    transports::Http,
    types::{H160, U256},
};

use crate::{configuration::Web3Settings, startup::SyncError};

pub struct Pool {
    pub contract: Contract<Http>,
}

impl Pool {
    pub fn new(config: &Web3Settings) -> Result<Self, SyncError> {
        let http = web3::transports::Http::new(&config.provider_endpoint)
            .expect("failed to init web3 provider");

        let web3 = web3::Web3::new(http);
        // let file = BufReader::new(File::open(&config.abi_path)?);

        let contract_address = H160::from_str(&config.pool_address).expect("bad pool address");

        // let contract = Contract::from_json(
        //     web3.eth(),
        //     contract_address,
        //     file.buffer(),
        // )
        // .expect("failed to init pool contract interface");


        let contract = Contract::from_json(
            web3.eth(),
            contract_address,
            include_bytes!("../configuration/pool-abi.json"),
        ).expect("failed to read contract");

        Ok(Self { contract })
    }
    pub async fn root(&self) -> Result<(U256, Num<Fr>), SyncError> {
        let contract = &self.contract;
        let result = contract.query("pool_index", (), None, Options::default(), None);
        let pool_index: U256 = result.await?;

        let result = contract.query("roots", (pool_index,), None, Options::default(), None);

        let root: U256 = result.await?;

        let root = Num::from_str(&root.to_string()).unwrap();

        tracing::debug!("got root from contract {}", root);

        Ok((pool_index, root))
    }
}
