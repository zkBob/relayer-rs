use crate::{
    custody::{
        tx_parser::ParseResult,
        types::{HistoryDbColumn, HistoryRecord},
    },
    helpers::BytesRepr,
    state::State,
    types::{
        job::Response,
        transaction_request::{Proof, TransactionRequest},
    },
};
use actix_web::web::Data;
use kvdb::KeyValueDB;
use kvdb_rocksdb::DatabaseConfig;
use libzeropool::fawkes_crypto::ff_uint::NumRepr;
use libzeropool::{
    circuit::tx::c_transfer,
    fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters},
    fawkes_crypto::{backend::bellman_groth16::prover::prove, ff_uint::Num},
};
use memo_parser::memo::TxType as MemoTxType;
use serde::{Serialize, Deserialize};
use std::{
    fs, thread,
    time::{Duration, SystemTime},
};
use tokio::sync::mpsc::{self, Receiver, Sender};
use uuid::Uuid;

use super::{
    account::Account,
    config::CustodyServiceSettings,
    errors::CustodyServiceError,
    tx_parser::{self, IndexedTx, TxParser},
    types::{AccountDetailedInfo, AccountShortInfo, Fr, RelayerState, TransferRequest},
};
use libzkbob_rs::{
    client::{TokenAmount, TxOutput, TxType},
    proof::prove_tx,
};
use std::str::FromStr;

pub enum CustodyDbColumn {
    JobsIndex,
    NullifierIndex,
}

impl Into<u32> for CustodyDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}

pub struct CustodyService {
    pub settings: CustodyServiceSettings,
    pub accounts: Vec<Account>,
    pub params: Parameters<Bn256>,
    pub db: kvdb_rocksdb::Database,
    pub sender: Sender<TransferRequest>,
    pub receiver: Receiver<TransferRequest>,
}

#[derive(Serialize,Deserialize)]
pub enum TransferStatus {
    New,
    Proving,
    Relaying,
    Mining,
    Done,
    Failed
}


#[derive(Serialize, Deserialize)]
pub struct JobShortInfo {
    // request_id: String,
    job_id: Option<Vec<u8>>,
    status: TransferStatus

}
impl CustodyService {
    pub fn start_request_processing(mut self) {
        tokio::task::spawn(async move {
            tracing::info!("starting tx_sender...");

            while let Some(request) = self.receiver.recv().await {
                match self.process_transfer_request(request).await {
                    Ok(_) => todo!(),
                    Err(e) => tracing::debug!("failed to process transfer request /n/t{:#?}", e),
                }
            }
        });
    }

    pub async fn process_transfer_request(
        &self,
        request: TransferRequest,
    ) -> Result<(), CustodyServiceError> {
        let request_id = request.request_id.as_ref().unwrap().clone();
        let transaction_request = vec![self.transfer(request)?];

        let relayer_endpoint = format!("{}/sendTransactions", self.settings.relayer_url);

        let response = reqwest::Client::new()
            .post(relayer_endpoint)
            .json(&transaction_request)
            .header("Content-type", "application/json")
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    "network exception when sending request to relayer: {:#?}",
                    e
                );
                CustodyServiceError::RelayerSendError
            })?;

        let response = response.error_for_status().map_err(|e| {
            tracing::error!("relayer returned bad status code {:#?}", e);
            CustodyServiceError::RelayerSendError
        })?;

        let response: Response = response.json().await.map_err(|e| {
            tracing::error!("the relayer response was not JSON: {:#?}", e);
            CustodyServiceError::RelayerSendError
        })?;

        // TODO: multitransfer
        let nullifier = transaction_request[0].proof.inputs[1].bytes();
        self.save_nullifier(&request_id, nullifier).map_err(|err| {
            tracing::error!("failed to save nullifier: {}", err);
            CustodyServiceError::DataBaseWriteError
        })?;

        self.update_request_status(&request_id, Some(&response.job_id.into_bytes()), TransferStatus::Relaying)
            .map_err(|err| {
                tracing::error!("failed to save job_id: {}", err);
                CustodyServiceError::DataBaseWriteError
            })?;

        tracing::info!("relayer returned the job id: {:#?}", response.job_id);
        Ok(())
    }
    pub fn new<D: KeyValueDB>(
        params: Parameters<Bn256>,
        settings: CustodyServiceSettings,
        state: Data<State<D>>,
    ) -> Self {
        let base_path = format!("{}/accounts_data", &settings.db_path);
        let mut accounts = vec![];

        let paths = fs::read_dir(&base_path);
        if let Ok(paths) = paths {
            tracing::info!("Loading accounts...");
            for path in paths {
                if let Ok(path) = path {
                    if path.file_type().unwrap().is_dir() {
                        let account_id = path.file_name();
                        let account_id = account_id.to_str().unwrap();
                        tracing::info!("Loading: {}", account_id);
                        let account = Account::load(&base_path, account_id).unwrap();
                        accounts.push(account);
                    }
                }
            }
        }

        let db = kvdb_rocksdb::Database::open(
            &DatabaseConfig {
                columns: 2,
                ..Default::default()
            },
            &format!("{}/custody", &settings.db_path),
        )
        .unwrap();

        let sync_interval = Duration::from_secs(settings.sync_interval_sec);
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            loop {
                tracing::debug!("Sync custody state...");
                rt.block_on(state.sync()).unwrap();
                thread::sleep(sync_interval);
            }
        });

        let (sender, mut receiver) = mpsc::channel::<TransferRequest>(100);

        Self {
            accounts,
            settings,
            params,
            db,
            sender,
            receiver,
        }
    }

    pub fn new_account(&mut self, description: String) -> Uuid {
        let base_path = format!("{}/accounts_data", self.settings.db_path);
        let account = Account::new(&base_path, description);
        let id = account.id;
        self.accounts.push(account);
        tracing::info!("created a new account: {}", id);
        id
    }

    pub fn gen_address(&self, account_id: Uuid) -> Option<String> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| {
                let account = account.inner.read().unwrap();
                account.generate_address()
            })
    }

    pub fn transfer(
        &self,
        request: TransferRequest,
    ) -> Result<TransactionRequest, CustodyServiceError> {
        let account_id = Uuid::from_str(&request.account_id).unwrap();
        let account = self.account(account_id)?;
        let account = account.inner.read().unwrap();

        let fee = 100000000;
        let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();

        let tx_output: TxOutput<Fr> = TxOutput {
            to: request.to,
            amount: TokenAmount::new(Num::from_uint(NumRepr::from(request.amount)).unwrap()),
        };
        let transfer = TxType::Transfer(TokenAmount::new(fee), vec![], vec![tx_output]);

        let tx = account.create_tx(transfer, None, None).unwrap();
        let (inputs, proof) = prove_tx(
            &self.params,
            &*libzeropool::POOL_PARAMS,
            tx.public,
            tx.secret,
        );

        let proof = Proof { inputs, proof };

        let tx_request = TransactionRequest {
            uuid: Some(Uuid::new_v4().to_string()),
            proof,
            memo: hex::encode(tx.memo),
            tx_type: format!("{:0>4}", MemoTxType::Transfer.to_u32()),
            deposit_signature: None,
        };

        Ok(tx_request)
    }

    pub fn deposit(
        &self,
        account_id: Uuid,
        amount: u64,
        holder: String,
        params: &Parameters<Bn256>,
    ) -> Result<TransactionRequest, CustodyServiceError> {
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .unwrap();

        let account = account.inner.read().unwrap();
        let deadline: u64 = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 1200;
        let fee: u64 = 100000000;

        let deposit_amount: Num<Fr> = Num::from_uint(NumRepr::from(amount + fee)).unwrap();
        let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();
        let holder = hex::decode(holder).unwrap();
        let deposit = TxType::DepositPermittable(
            TokenAmount::new_trimmed(fee),
            vec![],
            TokenAmount::new_trimmed(deposit_amount),
            deadline,
            holder,
        );
        let tx = account.create_tx(deposit, None, None).unwrap();

        let circuit = |public, secret| {
            c_transfer(&public, &secret, &*libzeropool::POOL_PARAMS);
        };

        let (inputs, snark_proof) = prove(params, &tx.public, &tx.secret, circuit);

        let proof = Proof {
            inputs,
            proof: snark_proof,
        };

        let tx_request = TransactionRequest {
            uuid: Some(Uuid::new_v4().to_string()),
            proof,
            memo: hex::encode(tx.memo),
            tx_type: MemoTxType::DepositPermittable.to_string(),
            deposit_signature: None,
        };

        Ok(tx_request)
    }

    pub fn account_info(&self, account_id: Uuid) -> Option<AccountShortInfo> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(Account::short_info)
    }

    pub fn list_accounts(&self) -> Vec<AccountShortInfo> {
        self.accounts.iter().map(Account::short_info).collect()
    }

    pub fn sync_account<D: KeyValueDB>(
        &self,
        account_id: Uuid,
        relayer_state: &RelayerState<D>,
    ) -> Result<(), CustodyServiceError> {
        tracing::info!("starting sync for account {}", account_id);
        let account = self.account(account_id)?;

        tracing::info!("account {} found", account_id);
        let start_index = account.next_index();
        let finalized_index = {
            let finalized = relayer_state.finalized.lock().unwrap();
            finalized.next_index()
        };
        tracing::info!(
            "account {}, account_index = {}, finalized index = {} ",
            account_id,
            start_index,
            finalized_index
        );

        // TODO: error instead of unwrap
        let jobs = relayer_state
            .get_jobs(start_index, finalized_index)
            .unwrap();

        let indexed_txs: Vec<IndexedTx> = jobs
            .iter()
            .map(|item| IndexedTx {
                index: item.index,
                memo: item.memo.clone(),
                commitment: item.commitment,
            })
            .collect();

        let job_indices: Vec<u64> = jobs.iter().map(|j| j.index).collect();

        tracing::info!("jobs to sync {:#?}", job_indices);

        let parse_result: ParseResult = TxParser::new().parse_native_tx(account.sk(), indexed_txs);

        tracing::info!(
            "retrieved new_accounts: {:#?} \n new notes: {:#?}",
            parse_result.state_update.new_accounts,
            parse_result.state_update.new_notes
        );
        let decrypted_memos = parse_result.decrypted_memos;

        let mut batch = account.history.transaction();
        decrypted_memos.iter().for_each(|memo| {
            let jobs = relayer_state.get_jobs(memo.index, 1).unwrap();
            let job = jobs.first().unwrap();
            let tx = job.transaction.as_ref().unwrap();

            let record = HistoryRecord {
                dec_memo: memo.clone(),
                tx_hash: tx.hash,
                calldata: tx.input.0.clone(),
                block_num: tx.block_number.unwrap(),
            };

            batch.put_vec(
                HistoryDbColumn::NotesIndex.into(),
                &tx_parser::index_key(memo.index),
                serde_json::to_vec(&record).unwrap(),
            );
        });

        account.history.write(batch).unwrap();
        tracing::info!("account {} saved history", account_id);

        account.update_state(parse_result.state_update);
        tracing::info!("account {} state updated", account_id);

        Ok(())
    }

    pub fn account(&self, account_id: Uuid) -> Result<&Account, CustodyServiceError> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or(CustodyServiceError::AccountNotFound)
    }

    pub fn update_request_status(&self, request_id: &str, job_id: Option<Vec<u8>>, status:TransferStatus ) -> Result<(), String> {
        let tx = {
            let mut tx = self.db.transaction();
            tx.put(
                CustodyDbColumn::JobsIndex.into(),
                request_id.as_bytes(),
                &serde_json::to_vec(&JobShortInfo {
                    job_id,
                    status
                }).unwrap(),
            );
            tx
        };
        self.db.write(tx).map_err(|err| err.to_string())
    }

    pub fn get_job_id_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Option<String>, CustodyServiceError> {
        self.db
            .get(CustodyDbColumn::JobsIndex.into(), request_id.as_bytes())
            .map_err(|err| {
                tracing::error!("failed to get job id from database: {}", err);
                CustodyServiceError::DataBaseReadError
            })?
            .map(|id| {
                String::from_utf8(id).map_err(|err| {
                    tracing::error!("failed to parse job id from database: {}", err);
                    CustodyServiceError::DataBaseReadError
                })
            })
            .map_or(Ok(None), |v| v.map(Some))
    }

    pub fn save_nullifier(&self, request_id: &str, nullifier: Vec<u8>) -> Result<(), String> {
        let tx = {
            let mut tx = self.db.transaction();
            tx.put(
                CustodyDbColumn::NullifierIndex.into(),
                &nullifier,
                request_id.as_bytes(),
            );
            tx
        };
        self.db.write(tx).map_err(|err| err.to_string())
    }

    pub fn get_request_id(&self, nullifier: Vec<u8>) -> Result<String, String> {
        let request_id = self
            .db
            .get(CustodyDbColumn::NullifierIndex.into(), &nullifier)
            .map_err(|err| err.to_string())?
            .ok_or("transaction id not found")?;

        let request_id = String::from_utf8(request_id).map_err(|err| err.to_string())?;

        Ok(request_id)
    }
}
