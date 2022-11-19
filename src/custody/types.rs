use crate::state::State;
use actix_web::web::Data;

use ethabi::ethereum_types::{H256, U64};
use libzeropool::{
    constants,
    fawkes_crypto::{core::sizedvec::SizedVec, ff_uint::Num, backend::bellman_groth16::{Parameters, engines::Bn256}},
    native::tx::{TransferPub, TransferSec},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use std::{fmt::Debug};

use libzkbob_rs::libzeropool::native::params::{PoolBN256, PoolParams as PoolParamsTrait};

use super::{account::Account, service::TransferStatus, tx_parser::DecMemo};

#[derive(Serialize)]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub balance: String,
}

#[derive(Serialize)]
pub struct AccountDetailedInfo {
    pub id: String,
    pub description: String,
    pub index: String,
    pub sync_status: bool,
    pub total_balance: String,
    pub account_balance: String,
    pub note_balance: String,
}

pub enum HistoryDbColumn {
    NotesIndex,
}

impl Into<u32> for HistoryDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}
pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;

pub type RelayerState<D> = Data<State<D>>;

#[derive(Serialize)]
struct TransactionData {
    public: TransferPub<Fr>,
    secret: TransferSec<Fr>,
    #[serde(with = "hex")]
    ciphertext: Vec<u8>,
    // #[serde(with = "hex")]
    memo: Vec<u8>,
    commitment_root: Num<Fr>,
    out_hashes: SizedVec<Num<Fr>, { constants::OUT + 1 }>,
    parsed_delta: ParsedDelta,
}
#[derive(Serialize)]
struct ParsedDelta {
    v: i64,
    e: i64,
    index: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SignupRequest {
    pub description: String,
}

#[derive(Deserialize)]
pub struct AccountInfoRequest {
    pub id: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub request_id: Option<String>,
    pub account_id: String,
    pub amount: u64,
    pub to: String,

    pub webhook: Option<String>,
}
#[derive(Clone)]
pub struct ScheduledTask {
    pub request_id: String,
    pub db: Data< kvdb_rocksdb::Database>,
    pub request: TransferRequest,
    pub job_id: Option<Vec<u8>>,
    pub endpoint: Option<String>,
    pub relayer_url: String,
    pub retries_left: u8,
    pub status: TransferStatus,
    pub tx_hash: Option<String>,
    pub failure_reason: Option<String>,
    pub account: Data<Account>,
    pub params: Data<Parameters<Bn256>>
}



impl Debug for ScheduledTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledTask")
            .field("request_id", &self.request_id)
            .field("request", &self.request)
            .field("job_id", &self.job_id)
            .field("endpoint", &self.endpoint)
            .field("relayer_url", &self.relayer_url)
            .field("retries_left", &self.retries_left)
            .field("status", &self.status)
            .field("tx_hash", &self.tx_hash)
            .field("failure_reason", &self.failure_reason)
            
            .finish()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAddressResponse {
    pub address: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupResponse {
    pub account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAccountsResponse {
    pub accounts: Vec<AccountShortInfo>,
}

#[derive(Serialize)]
pub enum HistoryTxType {
    Deposit,
    Withdrawal,
    TransferIn,
    TransferOut,
    ReturnedChange,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTx {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    pub timestamp: String,
    pub amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct HistoryRecord {
    pub dec_memo: DecMemo,
    pub tx_hash: H256,
    pub calldata: Vec<u8>,
    pub block_num: U64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResponse {
    pub request_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusRequest {
    pub request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusResponse {
    pub status: TransferStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}
