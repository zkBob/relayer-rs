use std::str::FromStr;

use libzeropool::fawkes_crypto::{engines::bn256::Fr, ff_uint::Num};
use web3::{
    contract::{Contract, Options},
    transports::Http,
    types::{H256, H160, U256, TransactionId, Transaction},
    Web3,
};

use actix_web::web::Data;

use crate::{configuration::Web3Settings, startup::SyncError};

pub struct Pool {
    pub contract: Contract<Http>,
    web3: Web3<Http>,
}

fn get_web3(config: &Data<Web3Settings>) -> web3::Web3<Http> {
    let http = web3::transports::Http::new(&config.provider_endpoint)
        .expect("failed to init web3 provider");

    web3::Web3::new(http)
}


impl Pool {

    pub async fn get_transaction(&self, tx_hash: H256) -> Result<Option<Transaction>, web3::Error> {

        self.web3.eth().transaction(TransactionId::Hash(tx_hash)).await
        // web3
    }

    pub fn new(config: Data<Web3Settings>) -> Result<Self, SyncError> {
        let contract_address = H160::from_str(&config.pool_address).expect("bad pool address");

        let web3 = get_web3(&config);
        let contract = Contract::from_json(
            web3.eth(),
            contract_address,
            include_bytes!("../configuration/pool-abi.json"),
        )
        .expect("failed to read contract");

        Ok(Self {
            contract,
            web3: web3.clone(),
        })
    }

    pub async fn check_nullifier(&self, nullifier: &str) -> Result<bool, web3::error::Error> {
        Ok(false)
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
