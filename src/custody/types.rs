use crate::state::State;
use actix_web::{
    web::Data,
};

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

#[derive(Serialize)]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub index: String,
    pub sync_status: bool,
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
    TxHashIndex,
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

// type RelayerState = Data<State<kvdb_rocksdb::Database>>;
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
    pub id: Option<String>,
    pub account_id: String,
    pub amount:u64,
    pub to: String,
}