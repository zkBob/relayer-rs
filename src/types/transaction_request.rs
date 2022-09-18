use actix_web::web::Data;
use kvdb::KeyValueDB;
use libzeropool::fawkes_crypto::{backend::bellman_groth16::{verifier, prover, engines::Bn256}, ff_uint::Num, engines::bn256::Fr};
use memo_parser::memo::{Memo, TxType};
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::{routes::{ServiceError}, state::State};

#[derive(Serialize, Deserialize)]
pub struct Proof {
    pub inputs: Vec<Num<Fr>>,
    pub proof: prover::Proof<Bn256>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRequest {
    pub uuid: Option<String>,
    pub proof: Proof,
    pub memo: String,
    pub tx_type: String,
    pub deposit_signature: Option<String>,
}

impl core::fmt::Debug for TransactionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("memo", &self.memo)
            .field("tx_type", &self.tx_type)
            .field("deposit_signature", &self.deposit_signature)
            .finish()
    }
}

impl TransactionRequest {
    pub async fn validate<D: KeyValueDB>(&self, state: &Data<State<D>>) -> Result<(), ServiceError> {
        let request_id = self
            .uuid
            .as_ref()
            .map(|id| Uuid::parse_str(&id).unwrap())
            .unwrap_or(Uuid::new_v4());
        
        self.check_relayer_fee(state, request_id)?;
        self.check_nullifier(state, request_id).await?;
        self.verify_tx_proof(state, request_id)?;
        // TODO: copy all checks from ts relayer
    
        Ok(())
    }
    
    fn check_relayer_fee<D: KeyValueDB>(
        &self, 
        state: &Data<State<D>>,
        request_id: Uuid,
    ) -> Result<(), ServiceError> {
        let tx_memo_bytes = hex::decode(&self.memo);
        if tx_memo_bytes.is_err() {
            return Err(ServiceError::BadRequest("Bad memo field!".to_owned()));
        }
        let tx_type = match self.tx_type.as_str() {
            "0000" => TxType::Deposit,
            "0001" => TxType::Transfer,
            "0002" => TxType::Withdrawal,
            "0003" => TxType::DepositPermittable,
            _ => TxType::Deposit,
        };
        let parsed_memo = Memo::parse_memoblock(&tx_memo_bytes.unwrap(), tx_type);
    
        if parsed_memo.fee < state.web3.relayer_fee {
            tracing::warn!(
                "request_id: {}, fee {:#?} , Fee too low!",
                request_id,
                parsed_memo.fee
            );
            return Err(ServiceError::BadRequest("Fee too low!".to_owned()));
        }
    
        Ok(())
    }
    
    async fn check_nullifier<D: KeyValueDB>(
        &self, 
        state: &Data<State<D>>,
        request_id: Uuid,
    ) -> Result<(), ServiceError> {
        let nullifier = self.proof.inputs[1];
        tracing::info!(
            "request_id: {}, Checking Nullifier {:#?}",
            request_id,
            nullifier
        );
        let pool = &state.pool;
        if !pool.check_nullifier(&nullifier.to_string()).await.unwrap() {
            let error_message = format!(
                "request_id: {}, Nullifier {:#?} , Double spending detected",
                request_id, &nullifier
            );
            tracing::warn!("{}", error_message);
            return Err(ServiceError::BadRequest(error_message));
        }
    
        Ok(())
    }
    
    fn verify_tx_proof<D: KeyValueDB>(
        &self, 
        state: &Data<State<D>>,
        _request_id: Uuid,
    ) -> Result<(), ServiceError> {
        if !verifier::verify(
            &state.vk,
            &self.proof.proof,
            &self.proof.inputs,
        ) {
            tracing::info!("received bad proof");
            return Err(ServiceError::BadRequest("Invalid proof".to_owned()));
        }
    
        Ok(())
    }
}