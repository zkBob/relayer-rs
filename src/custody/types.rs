use crate::state::State;
use actix_web::{
    web::Data,
};

use ethabi::ethereum_types::{H256, U64};
use libzeropool::{
    constants,
    fawkes_crypto::{
        core::sizedvec::SizedVec,
        ff_uint::Num,
    },
    native::tx::{TransferPub, TransferSec},
};

use serde::{Deserialize, Serialize};

use libzkbob_rs::libzeropool::native::params::{PoolBN256, PoolParams as PoolParamsTrait};

use super::tx_parser::DecMemo;

#[derive(Serialize)]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub index: String,
    pub sync_status: bool,
}

#[derive(Serialize)]
pub struct AccountDetailedInfo {
    pub success: bool,
    pub id: String,
    pub description: String,
    pub index: String,
    pub sync_status: bool,
    pub total_balance: String,
    pub account_balance: String,
    pub note_balance: String, 
}

pub enum HistoryDbColumn {
    NullifierIndex,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub id: String,
    pub account_id: String,
    pub amount:u64,
    pub to: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAddressResponse {
    pub success: bool,
    pub address: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResponse {
    pub success: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupResponse {
    pub success: bool,
    pub account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAccountsResponse {
    pub success: bool,
    pub accounts: Vec<AccountShortInfo>,
}

#[derive(Serialize)]
pub enum HistoryTxType {
    Deposit,
    Withdrawal,
    TransferIn,
    TransferOut,
    TransferLoopback,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTx {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    pub tx_index: String,
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
pub struct HistoryResponse {
    pub success: bool,
    pub txs: Vec<HistoryTx>
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResponse {
    pub success: bool,
    pub transaction_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusRequest {
    pub transaction_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusResponse {
    pub success: bool,
    pub state: String,
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_reason: Option<String>,
}



