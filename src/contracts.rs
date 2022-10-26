use std::str::FromStr;

use libzeropool::fawkes_crypto::{engines::bn256::Fr, ff_uint::Num};
use secp256k1::SecretKey;
use web3::{
    contract::{Contract, Options},
    transports::Http,
    types::{
        BlockNumber, Bytes, FilterBuilder, Log, LogWithMeta, Transaction, TransactionId,
        TransactionReceipt, H160, H256, U256,
    },
    Error as Web3Error, Web3,
};

use actix_web::web::Data;

use crate::{configuration::Web3Settings, state::SyncError};

type MessageEvent = (U256, H256, Bytes);
type Events = Vec<LogWithMeta<MessageEvent>>;

pub struct Pool {
    pub contract: Contract<Http>,
    web3: Web3<Http>,

    key: SecretKey,
    gas_limit: U256,
    transact_short_signature: Vec<u8>,
}

impl Pool {
    pub fn new(config: &Web3Settings) -> Result<Self, SyncError> {
        let contract_address = H160::from_str(&config.pool_address).expect("bad pool address");

        let http = web3::transports::Http::new(&config.provider_endpoint)
            .expect("failed to init web3 provider");
        let web3 = web3::Web3::new(http);

        let contract = Contract::from_json(
            web3.eth(),
            contract_address,
            include_bytes!("../configuration/pool-abi.json"),
        )
        .expect("failed to read contract");

        let key =
            SecretKey::from_str(&config.credentials.secret_key).expect("failed to read secret key");

        let short_signature = contract
            .abi()
            .function("transact")
            .unwrap()
            .short_signature()
            .to_vec();

        Ok(Self {
            contract,
            web3: web3.clone(),
            key,
            gas_limit: U256::from(config.gas_limit),
            transact_short_signature: short_signature,
        })
    }

    pub async fn get_transaction(&self, tx_hash: H256) -> Result<Option<Transaction>, web3::Error> {
        self.web3
            .eth()
            .transaction(TransactionId::Hash(tx_hash))
            .await
    }

    pub async fn get_transaction_receipt(
        &self,
        tx_hash: H256,
    ) -> Result<Option<TransactionReceipt>, web3::Error> {
        self.web3.eth().transaction_receipt(tx_hash).await
    }

    pub async fn check_nullifier(&self, nullifier: &str) -> Result<bool, web3::error::Error> {
        let exists: U256 = self
            .contract
            .query(
                "nullifiers",
                (U256::from_dec_str(nullifier).unwrap(),),
                None,
                Options::default(),
                None,
            )
            .await
            .expect("failed to check nullifier");
        Ok(exists.is_zero())
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

    pub async fn get_events(
        &self,
        from_block: Option<BlockNumber>,
        to_block: Option<BlockNumber>,
        block_hash: Option<H256>,
    ) -> Result<Events, SyncError> {
        let result = self
            .contract
            .events("Message", from_block, to_block, block_hash, (), (), ());

        let events: Events = result.await?;

        Ok(events)
    }

    pub async fn get_logs(&self) -> Result<Vec<Log>, SyncError> {
        let res = self.contract.abi().event("Message").and_then(|ev| {
            let filter = ev.filter(ethabi::RawTopicFilter {
                topic0: ethabi::Topic::Any,
                topic1: ethabi::Topic::Any,
                topic2: ethabi::Topic::Any,
            })?;
            Ok((ev.clone(), filter))
        });
        let (_ev, filter) = match res {
            Ok(x) => x,
            Err(_e) => return Err(SyncError::GeneralError("WTF".to_string())),
        };

        let address = self.contract.address();
        tracing::info!("filter {:#?}", filter);
        tracing::info!("address {:#?}", address);

        let logs = self
            .web3
            .eth()
            .logs(
                FilterBuilder::default()
                    .address(vec![self.contract.address()])
                    .topic_filter(filter)
                    .from_block(Some(BlockNumber::Earliest))
                    .to_block(Some(BlockNumber::Latest))
                    .block_hash(None)
                    .build(),
            )
            .await
            .unwrap();

        // std::fs::write(
        //     "tests/data/logs.json",
        //     serde_json::to_string_pretty(&logs).unwrap(),
        // )
        // .unwrap();

        Ok(logs)
    }

    pub async fn send_tx(&self, tx_data: Vec<u8>) -> Result<H256, String> {
        let fn_data: Vec<u8> = vec![self.transact_short_signature.clone(), tx_data].concat();

        let gas_price = self.gas_price().await.map_err(|e| e.to_string())?;

        let options = Options {
            gas: Some(self.gas_limit),
            gas_price: Some(gas_price),
            ..Default::default()
        };

        let tx_hash = self
            .contract
            .signed_call_raw(fn_data, options, &self.key)
            .await
            .map_err(|e| e.to_string())?;

        Ok(tx_hash)
    }

    async fn gas_price(&self) -> Result<U256, Web3Error> {
        self.web3.eth().gas_price().await
    }
}
