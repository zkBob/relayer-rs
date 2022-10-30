use actix_web::web::Data;
use libzeropool::native::params::PoolParams;

use crate::state::State;

use super::{account::Account, tx_parser::{IndexedTx, TxParser}};

pub struct CustodyService {
    accounts: Vec<Account>,
}

impl CustodyService {
    pub fn new_account(&mut self, description: String) {
        self.accounts.push(Account::new(description));
    }

    pub fn sync_account(
        &self,
        account_id: String,
        relayer_state: Data<State<kvdb_rocksdb::Database>>,
    ) {
        match self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
        {
            Some(account) => {
                let start_index = account.user_account.state.tree.next_index();
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

                    let parse_result = TxParser::new().parse_native_tx(account.user_account.keys.sk, indexed_txs);

            }
            None => todo!(),
        }
        // relayer_state.get_jobs(offset, limit)
        ()
    }
}
