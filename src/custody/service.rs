use std::borrow::BorrowMut;

use actix_web::web::Data;
use borsh::BorshSerialize;
use kvdb::{DBKey, DBOp, DBTransaction};
use libzeropool::native::params::PoolParams;
use serde::Serialize;
use uuid::Uuid;

use crate::{helpers, state::State};

use super::{
    account::Account,
    tx_parser::{self, IndexedTx, TxParser},
};

pub struct CustodyService {
    accounts: Vec<Account>,
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

impl CustodyService {
    pub fn new_account(&mut self, description: String) {
        self.accounts.push(Account::new(description));
    }

    pub fn sync_account(
        &self,
        account_id: Uuid,
        relayer_state: Data<State<kvdb_rocksdb::Database>>,
    ) {
        if let Some(account) = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
        {
            let start_index = account.next_index();

            let finalized = relayer_state.finalized.lock().unwrap();
            let finalized_index = finalized.next_index();
            let batch_size = 10 as u64; //TODO: loop
            let jobs = relayer_state
                .get_jobs(start_index, start_index + batch_size)
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

            let parse_result =
                TxParser::new().parse_native_tx(account.sk(), indexed_txs);

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

            let state_update = parse_result.state_update;
            let leafs = state_update.new_leafs;
            let commitments = state_update.new_commitments;

            account
                .user_account
                .as_ref()
                .borrow_mut()
                .state
                .tree
                .add_leafs_and_commitments(vec![], vec![]);
        }

        // let mut tree = account.user_account.state.tree.borrow_mut();
        // match maybe_acc
        // {
        //     Some(&mut account) => {
        //         let start_index = account.user_account.state.tree.next_index();
        //         let finalized = relayer_state.finalized.lock().unwrap();
        //         let finalized_index = finalized.next_index();
        //         let batch_size = 10 as u64; //TODO: loop
        //         let jobs = relayer_state
        //             .get_jobs(start_index, start_index + batch_size)
        //             .unwrap();

        //         let indexed_txs: Vec<IndexedTx> = jobs
        //             .iter()
        //             .enumerate()
        //             .map(|item| IndexedTx {
        //                 index: item.0 as u64,
        //                 memo: (item.1).memo.clone(),
        //                 commitment: (item.1).commitment,
        //             })
        //             .collect();

        //         let parse_result =
        //             TxParser::new().parse_native_tx(account.user_account.keys.sk, indexed_txs);

        //         let decrypted_memos = parse_result.decrypted_memos;

        //         let mut batch = account.history.transaction();
        //         decrypted_memos.iter().for_each(|memo| {
        //             batch.put_vec(
        //                 HistoryDbColumn::NotesIndex.into(),
        //                 &tx_parser::index_key(memo.index),
        //                 serde_json::to_vec(memo).unwrap()
        //             );
        //         });

        //         account.history.write(batch).unwrap();

        //         let state_update = parse_result.state_update;
        //         let leafs = state_update.new_leafs;
        //         let commitments = state_update.new_commitments;
        //         let mut tree = account.user_account.state.tree.borrow_mut();
        //         tree.add_leafs_and_commitments(leafs, commitments);
        //     }
        //     None => todo!(),
        // }
        // relayer_state.get_jobs(offset, limit)
        ()
    }
}
