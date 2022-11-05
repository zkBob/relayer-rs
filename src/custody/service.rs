use crate::{
    custody::{tx_parser::ParseResult, types::HistoryDbColumn},
    routes::ServiceError,
    types::transaction_request::{Proof, TransactionRequest},
};
use kvdb::KeyValueDB;
use libzeropool::fawkes_crypto::ff_uint::NumRepr;
use libzeropool::{
    circuit::tx::c_transfer,
    fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters},
    fawkes_crypto::{backend::bellman_groth16::prover::prove, ff_uint::Num},
};
use memo_parser::memo::TxType as MemoTxType;
use std::time::SystemTime;
use uuid::Uuid;

use super::{
    account::Account,
    tx_parser::{self, IndexedTx, TxParser},
    types::{AccountShortInfo, Fr, RelayerState},
};
use libzkbob_rs::client::{TokenAmount, TxOutput, TxType};

pub struct CustodyService {
    pub accounts: Vec<Account>,
}

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

    pub fn trasfer(
        &self,
        account_id: Uuid,
        amount: u64,
        recipient_address: String,
        params: &Parameters<Bn256>,
    ) -> Result<TransactionRequest, ServiceError> {
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .unwrap();
        let account = account.inner.lock().unwrap();

        let fee = 100000000;
        let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();

        let tx_output: TxOutput<Fr> = TxOutput {
            to: recipient_address,
            amount: TokenAmount::new_trimmed(Num::from_uint(NumRepr::from(amount)).unwrap()),
        };
        let transfer = TxType::Transfer(TokenAmount::new_trimmed(fee), vec![], vec![tx_output]);

        let tx = account.create_tx(transfer, None, None).unwrap();

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
            tx_type: MemoTxType::Transfer.to_string(),
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
    ) -> Result<TransactionRequest, ServiceError> {
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .unwrap();

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
