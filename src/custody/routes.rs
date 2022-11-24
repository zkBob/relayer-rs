use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use kvdb::KeyValueDB;
use kvdb_rocksdb::Database;
use libzeropool::fawkes_crypto::{
    backend::bellman_groth16::{engines::Bn256, Parameters},
    ff_uint::{Num, NumRepr},
};
use libzkbob_rs::client::{TokenAmount, TransactionData, TxOutput, TxType};
use std::str::FromStr;
use tokio::sync::{mpsc::Sender, RwLock};
use uuid::Uuid;

use crate::{
    custody::{types::TransferResponse, scheduled_task::TransferStatus},
    routes::job::JobResponse,
    state::State
};

use super::{
    errors::CustodyServiceError,
    service::{CustodyService, JobShortInfo, JobStatusCallback, CustodyDbColumn},
    types::{
        AccountInfoRequest, Fr, GenerateAddressResponse, SignupRequest,
        SignupResponse, TransactionStatusResponse, TransferRequest, TransferStatusRequest,
    }, scheduled_task::ScheduledTask,
};

pub type Custody = Data<RwLock<CustodyService>>;

pub async fn account_info<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    custody.sync_account(account_id, &state).await?;

    let account_info = custody
        .account_info(account_id)
        .await
        .ok_or(CustodyServiceError::AccountNotFound)?;

    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    custody: Data<RwLock<CustodyService>>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CustodyServiceError> {
    let mut custody = custody.write().await;
    custody.validate_token(bearer.token())?;

    let account_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn list_accounts<D: KeyValueDB>(
    custody: Custody,
    bearer: BearerAuth,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;
    custody.validate_token(bearer.token())?;

    Ok(HttpResponse::Ok().json(custody.list_accounts().await))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    state: Data<State<D>>,
    custody: Custody,
    params: Data<Parameters<Bn256>>,
    custody_db: Data<Database>,
    prover_sender: Data<Sender<ScheduledTask>>,
    callback_sender: Data<Sender<JobStatusCallback>>,
) -> Result<HttpResponse, CustodyServiceError> {
    let request: TransferRequest = request.0.into();

    let account_id = Uuid::from_str(&request.account_id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody_clone = custody.clone();
    let custody = custody.read().await;
    let relayer_url = custody.settings.relayer_url.clone();
    custody.sync_account(account_id, &state).await?;
    
    let request_id = request
        .request_id
        .clone()
        .unwrap_or(Uuid::new_v4().to_string());
    if custody.has_request_id(&request_id)? {
        return Err(CustodyServiceError::DuplicateTransactionId);
    }

    let account = custody.account(account_id)?;
    let account = account.inner.read().await;

    let fee: u64 = 100000000;
    let fee: Num<Fr> = Num::from_uint(NumRepr::from(fee)).unwrap();

    let tx_output: TxOutput<Fr> = TxOutput {
        to: request.to.clone(),
        amount: TokenAmount::new(Num::from_uint(NumRepr::from(request.amount)).unwrap()),
    };
    let transfer = TxType::Transfer(TokenAmount::new(fee), vec![], vec![tx_output]);

    let tx: TransactionData<Fr> = account
        .create_tx(transfer, None, None)
        .map_err(|e| CustodyServiceError::BadRequest(e.to_string()))?;

    let db = custody_db.clone();

    tracing::info!(
        "{} request received & saved, tx created and sent to the prover queue",
        &request_id
    );
    let mut task = ScheduledTask {
        request_id: request_id.clone(),
        task_index: 0,
        account_id,
        job_id: None,
        endpoint: None,
        retries_left: 42,
        status: TransferStatus::New,
        tx_hash: None,
        failure_reason: None,
        relayer_url,
        params,
        db,
        tx: Some(tx),
        callback_address: request.webhook, // custody,
        callback_sender,
        amount: Num::from_uint(NumRepr::from(request.amount)).unwrap(),
        to: request.to.clone(),
        custody: custody_clone,
    };

    task.update_status(TransferStatus::New).await?;
    custody.save_tasks_count(&request_id, 1)?;

    prover_sender.send(task).await.unwrap();

    Ok(HttpResponse::Ok().json(TransferResponse {
        request_id: request_id.clone(),
    }))
}

pub async fn fetch_tx_status(
    relayer_endpoint: &str,
) -> Result<TransactionStatusResponse, CustodyServiceError> {
    let response = reqwest::Client::new()
        .get(relayer_endpoint)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "network exception when sending request to relayer: {:#?}",
                e
            );
            CustodyServiceError::RelayerSendError
        })?;

    let body = response.text().await.unwrap();

    match serde_json::from_str::<JobResponse>(&body) {
        Ok(response) => Ok(TransactionStatusResponse {
            status: TransferStatus::from(response.state),
            tx_hash: response.tx_hash,
            failure_reason: response.failed_reason,
        }),
        Err(e) => {
            tracing::error!("the relayer response {:#?} was not JSON: {:#?}", body, e);
            Err(CustodyServiceError::RelayerSendError)
        }
    }
}

pub async fn transaction_status<D: KeyValueDB>(
    request: Query<TransferStatusRequest>,
    _: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;

    let task_keys = custody.task_keys(&request.0.request_id)?;
    let mut jobs = vec![];
    for task_key in task_keys {
        let job = custody
            .db
            .get(
                CustodyDbColumn::JobsIndex.into(),
                &task_key,
            )
            .map_err(|_| CustodyServiceError::DataBaseReadError)?
            .ok_or(CustodyServiceError::TransactionNotFound)?;

        let job: JobShortInfo =
            serde_json::from_slice(&job).map_err(|_| CustodyServiceError::DataBaseReadError)?;
        jobs.push(job);
    }

    Ok(HttpResponse::Ok().json(jobs))
}

pub async fn generate_shielded_address<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    let account = custody.account(account_id)?;
    let address = account.generate_address().await;

    Ok(HttpResponse::Ok().json(GenerateAddressResponse { address }))
}

pub async fn history<D: KeyValueDB>(
    request: Query<AccountInfoRequest>,
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let account_id = Uuid::from_str(&request.id).map_err(|err| {
        tracing::error!("failed to parse account id: {}", err);
        CustodyServiceError::IncorrectAccountId
    })?;

    let custody = custody.read().await;

    custody.sync_account(account_id, &state).await?;

    let account = custody.account(account_id)?;
    let txs = account
        .history(&state.pool, |nullifier: Vec<u8>| {
            custody.get_request_id(nullifier)
        })
        .await;

    Ok(HttpResponse::Ok().json(txs))
}
