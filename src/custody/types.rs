use crate::state::State;

use ethabi::ethereum_types::{H256, U64, U256};
use serde::{Deserialize, Serialize};
use web3::types::LogWithMeta;
use std::fmt::Debug;

use libzkbob_rs::{libzeropool::native::params::{PoolBN256, PoolParams as PoolParamsTrait}};

use super::{tx_parser::DecMemo, scheduled_task::TransferStatus};

pub type ContractEvent = LogWithMeta<(U256,H256,H256)>;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JobShortInfo {
    pub status: TransferStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub balance: u64,
    pub single_tx_limit: u64
}

pub enum HistoryDbColumn {
    NotesIndex,
    BlockTimestampsCache
}

impl Into<u32> for HistoryDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}
pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;

pub type RelayerState<D> = State<D>;

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

#[derive(Deserialize,Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub request_id: Option<String>,
    pub account_id: String,
    pub amount: u64,
    pub to: String,

    pub webhook: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAddressResponse {
    pub address: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupResponse {
    pub account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAccountsResponse {
    pub accounts: Vec<AccountShortInfo>,
}

#[derive(Serialize, PartialEq, Clone)]
pub enum HistoryTxType {
    Deposit,
    Withdrawal,
    TransferIn,
    TransferOut,
    ReturnedChange,
    AggregateNotes,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTx {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    pub timestamp: u64,
    pub amount: u64,
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResponse {
    pub request_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculateFeeResponse {
    pub transaction_count: u64,
    pub total_fee: u64
}
#[derive(Deserialize,Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusRequest {
    pub request_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusResponse {
    pub status: TransferStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustodyTransactionStatusResponse {
    pub status: TransferStatus,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_tx_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

impl From<Vec<JobShortInfo>> for CustodyTransactionStatusResponse {
    fn from(jobs: Vec<JobShortInfo>) -> Self {
        let mut tx_hashes = jobs
            .iter()
            .filter(|job| job.tx_hash.is_some() && job.status != TransferStatus::Mining)
            .map(|job| job.tx_hash.clone().unwrap())
            .collect::<Vec<_>>();

        let tx_hash = tx_hashes.pop();
        let linked_tx_hashes = if tx_hash.is_some() {
            Some(tx_hashes)
        } else {
            None
        };

        let (status, timestamp, failure_reason) = {
            let last_job = jobs.last().unwrap();
            match last_job.status {
                TransferStatus::Done => (TransferStatus::Done, last_job.timestamp, None),
                TransferStatus::Failed(_) => {
                    let first_failed_job = jobs
                        .iter()
                        .filter(|job| {
                            match job.status {
                                TransferStatus::Failed(_) => true,
                                _ => false
                            }
                        })
                        .collect::<Vec<_>>()
                        .first()
                        .unwrap()
                        .clone();

                    (first_failed_job.status.clone(), first_failed_job.timestamp, first_failed_job.failure_reason.clone())
                },
                _ => {
                    let relevant_job = jobs
                        .iter()
                        .filter(|job| {
                            job.status != TransferStatus::Queued
                        })
                        .last()
                        .unwrap();
                    (TransferStatus::Relaying, relevant_job.timestamp, None)
                }
            }
        };

        CustodyTransactionStatusResponse {
            status,
            timestamp,
            tx_hash,
            linked_tx_hashes,
            failure_reason,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustodyHistoryRecord {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_tx_hashes: Option<Vec<String>>,
    pub timestamp: u64,
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}

impl CustodyHistoryRecord {
    pub fn convert_vec(txs: Vec<HistoryTx>) -> Vec<Self> {
        txs
            .iter()
            .filter(|tx| tx.tx_type != HistoryTxType::AggregateNotes)
            .map(|tx| {
                match tx.transaction_id.clone() {
                    Some(request_id) => {
                        let linked_tx_hashes = txs
                            .iter()
                            .filter(|tx| tx.transaction_id == Some(request_id.clone()))
                            .filter(|tx| tx.tx_type == HistoryTxType::AggregateNotes)
                            .map(|linked_tx| {
                                linked_tx.tx_hash.clone()
                            }).collect::<Vec<_>>();
                        let linked_tx_hashes = if linked_tx_hashes.len() > 0 { Some(linked_tx_hashes) } else { None };
                        
                        CustodyHistoryRecord {
                            tx_type: tx.tx_type.clone(),
                            tx_hash: tx.tx_hash.clone(),
                            linked_tx_hashes,
                            timestamp: tx.timestamp.clone(),
                            amount: tx.amount.clone(),
                            to: tx.to.clone(),
                            transaction_id: Some(request_id),
                        }
                    }
                    None => {
                        CustodyHistoryRecord {
                            tx_type: tx.tx_type.clone(),
                            tx_hash: tx.tx_hash.clone(),
                            linked_tx_hashes: None,
                            timestamp: tx.timestamp.clone(),
                            amount: tx.amount.clone(),
                            to: tx.to.clone(),
                            transaction_id: None,
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CalculateFeeRequest {
    pub account_id: String,
    pub amount: u64,
}