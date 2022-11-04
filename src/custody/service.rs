use crate::{
    custody::tx_parser::ParseResult,
    helpers,
    routes::ServiceError,
    state::{State, DB},
};
use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use kvdb::KeyValueDB;
use libzeropool::{
    fawkes_crypto::ff_uint::{Num, Uint},
    native::boundednum::BoundedNum,
};
use libzeropool::{
    fawkes_crypto::{engines::U256, ff_uint::NumRepr},
    POOL_PARAMS,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, str::FromStr, sync::Mutex, time::SystemTime};
use uuid::Uuid;

use libzkbob_rs::{
    client::TokenAmount,
    libzeropool::native::params::{PoolBN256, PoolParams as PoolParamsTrait},
};

pub type PoolParams = PoolBN256;
pub type Fr = <PoolParams as PoolParamsTrait>::Fr;
pub type Fs = <PoolParams as PoolParamsTrait>::Fs;

use super::{
    account::Account,
    tx_parser::{self, IndexedTx, TxParser},
};

pub struct CustodyService {
    accounts: Vec<Account>,
}

#[derive(Serialize, Deserialize)]
pub struct SignupRequest {
    description: String,
}

#[derive(Deserialize)]
pub struct AccountInfoRequest {
    id: String,
}

#[derive(Serialize)]
pub struct AccountShortInfo {
    id: String,
    description: String,
    index: String,
    sync_status: bool,
}
pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    _state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let mut custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;
    let acc_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().body(acc_id.to_string()))
}

pub async fn sync_account<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    relayer_state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).unwrap();

    custody.sync_account(account_id, relayer_state);

    Ok(HttpResponse::Ok().finish())
}
pub async fn account_sync_status<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let account_id = Uuid::from_str(&request.id).unwrap();

    let state = state.finalized.lock().unwrap();
    let relayer_index = state.next_index();

    custody
        .sync_status_inner(account_id, relayer_index)
        .map_or(Ok(HttpResponse::NotFound().finish()), |res| {
            Ok(HttpResponse::Ok().json(res))
        })
}

pub async fn list_accounts<D: KeyValueDB>(
    state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>,
) -> Result<HttpResponse, ServiceError> {
    let custody = custody.lock().map_err(|_| {
        tracing::error!("failed to lock custody service");
        ServiceError::InternalError
    })?;

    let finalized = state.finalized.lock().unwrap();

    Ok(HttpResponse::Ok().json(custody.list_accounts(finalized.next_index())))
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

// type RelayerState = Data<State<kvdb_rocksdb::Database>>;
type RelayerState<D> = Data<State<D>>;

impl CustodyService {
    pub fn new() -> Self {
        Self { accounts: vec![] }
    }

    pub fn new_account(&mut self, description: String) -> Uuid {
        let account = Account::new(description);
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
                let account = account.inner.lock().unwrap();
                account.generate_address()
            })
    }

    /*
    if (fromAddress) {
      let deadline = Math.floor(Date.now() / 1000) + PERMIT_DEADLINE_INTERVAL;
      const holder = ethAddrToBuf(fromAddress);
      txData = await state.account.createDepositPermittable({
        amount: (amountGwei + feeGwei).toString(),
        fee: feeGwei.toString(),
        deadline: String(deadline),
        holder
      });

      // permittable deposit signature should be calculated for the typed data
      const value = (amountGwei + feeGwei) * state.denominator;
      const salt = '0x' + toTwosComplementHex(BigInt(txData.public.nullifier), 32);
      let signature = truncateHexPrefix(await signTypedData(BigInt(deadline), value, salt));
      if (this.config.network.isSignatureCompact()) {
        signature = toCompactSignature(signature);
      }

      // We should check deadline here because the user could introduce great delay
      if (Math.floor(Date.now() / 1000) > deadline - PERMIT_DEADLINE_THRESHOLD) {
        throw new TxDepositDeadlineExpiredError(deadline);
      }

      const startProofDate = Date.now();
      const txProof = await this.worker.proveTx(txData.public, txData.secret);
      const proofTime = (Date.now() - startProofDate) / 1000;
      console.log(`Proof calculation took ${proofTime.toFixed(1)} sec`);

      const txValid = Proof.verify(this.snarkParams.transferVk!, txProof.inputs, txProof.proof);
      if (!txValid) {
        throw new TxProofError();
      }

      let tx = { txType: TxType.BridgeDeposit, memo: txData.memo, proof: txProof, depositSignature: signature };
      const jobId = await this.sendTransactions(token.relayerUrl, [tx]);

      // Temporary save transaction in the history module (to prevent history delays)
      const ts = Math.floor(Date.now() / 1000);
      let rec = HistoryRecord.deposit(fromAddress, amountGwei, feeGwei, ts, "0", true);
      state.history.keepQueuedTransactions([rec], jobId);

     */

    pub fn deposit(&self, account_id: Uuid, amount: u64, signature: String, holder: String) -> Result<(), ServiceError> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| {
                let account = account.inner.lock().unwrap();
                let deadline: u64 = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 1200;
                let fee: u64 = 100000000;

                let deposit_amount: Num<Fr> = Num::from_uint(NumRepr::from(amount + fee)).unwrap();
                let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();
                let holder = hex::decode(holder).unwrap();
                let deposit = libzkbob_rs::client::TxType::DepositPermittable(
                    TokenAmount::new_trimmed(fee),
                    vec![],
                    TokenAmount::new_trimmed(deposit_amount),
                    deadline,
                    holder,
                );
                account.create_tx(deposit , None, None).unwrap();
                
            });
            Ok(())
    }

    pub fn sync_status_inner(
        &self,
        account_id: Uuid,
        relayer_index: u64,
    ) -> Option<AccountShortInfo> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| {
                let account_state = account.inner.lock().unwrap();
                AccountShortInfo {
                    id: account_id.to_string(),
                    description: account.description.clone(),
                    index: account_state.state.tree.next_index().to_string(),
                    sync_status: relayer_index == account_state.state.tree.next_index(),
                }
            })
    }

    pub fn list_accounts(&self, relayer_index: u64) -> Vec<AccountShortInfo> {
        self.accounts
            .iter()
            // .find(|account| account.id == account_id)
            .map(|account| {
                let account_state = account.inner.lock().unwrap();
                AccountShortInfo {
                    id: account.id.to_string(),
                    description: account.description.clone(),
                    index: account_state.state.tree.next_index().to_string(),
                    sync_status: relayer_index == account_state.state.tree.next_index(),
                }
            })
            .collect()
    }

    pub fn sync_account<D: KeyValueDB>(&self, account_id: Uuid, relayer_state: RelayerState<D>) {
        tracing::info!("starting sync for account {}", account_id);
        if let Some(account) = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
        {
            tracing::info!("account {} found", account_id);
            let start_index = account.next_index();

            let finalized = relayer_state.finalized.lock().unwrap();
            let finalized_index = finalized.next_index();
            tracing::info!(
                "account {}, finalized index = {} ",
                account_id,
                finalized_index
            );
            // let batch_size = ??? as u64; //TODO: loop
            let jobs = relayer_state
                .get_jobs(start_index, finalized_index)
                .unwrap();

            let indexed_txs: Vec<IndexedTx> = jobs
                .iter()
                .enumerate()
                .map(|item| IndexedTx {
                    index: item.0 as u64,
                    memo: (item.1).memo.clone(),
                    commitment: (item.1).commitment,
                })
                .collect();

            let job_indices: Vec<u64> = jobs.iter().map(|j| j.index).collect();

            tracing::info!("jobs to sync {:#?}", job_indices);

            let parse_result: ParseResult =
                TxParser::new().parse_native_tx(account.sk(), indexed_txs);

            tracing::info!(
                "retrieved new_accounts: {:#?} \n new notes: {:#?}",
                parse_result.state_update.new_accounts,
                parse_result.state_update.new_notes
            );
            let decrypted_memos = parse_result.decrypted_memos;

            let mut batch = account.history.transaction();
            decrypted_memos.iter().for_each(|memo| {
                batch.put_vec(
                    HistoryDbColumn::NotesIndex.into(),
                    &tx_parser::index_key(memo.index),
                    serde_json::to_vec(memo).unwrap(),
                );
            });

            account.history.write(batch).unwrap();
            tracing::info!("account {} saved history", account_id);

            account.update_state(parse_result.state_update);
            tracing::info!("account {} state updated", account_id);
        }
        ()
    }
}
