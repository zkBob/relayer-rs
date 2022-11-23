use actix_web::{
    web::{Data, Json, Query},
    HttpResponse,
};
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

use crate::{custody::types::TransferResponse, routes::job::JobResponse, state::State};

use super::{
    errors::CustodyServiceError,
    service::{CustodyDbColumn, CustodyService, JobShortInfo, JobStatusCallback, TransferStatus},
    types::{
        AccountInfoRequest, Fr, GenerateAddressResponse, ScheduledTask, SignupRequest,
        SignupResponse, TransactionStatusResponse, TransferRequest, TransferStatusRequest,
    },
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

    state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        CustodyServiceError::StateSyncError
    })?;

    custody.sync_account(account_id, &state).await?;

    let account_info = custody
        .account_info(account_id)
        .await
        .ok_or(CustodyServiceError::AccountNotFound)?;

    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn signup<D: KeyValueDB>(
    request: Json<SignupRequest>,
    _state: Data<State<D>>,
    custody: Data<RwLock<CustodyService>>,
    _params: Data<Parameters<Bn256>>,
    _custody_db: Data<Database>,
    _prover_sender: Data<Sender<ScheduledTask>>,
) -> Result<HttpResponse, CustodyServiceError> {
    let mut custody = custody.write().await;

    let account_id = custody.new_account(request.0.description);

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn list_accounts<D: KeyValueDB>(
    state: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;

    state.sync().await.map_err(|_| {
        tracing::error!("failed to sync state");
        CustodyServiceError::StateSyncError
    })?;

    Ok(HttpResponse::Ok().json(custody.list_accounts().await))
}

pub async fn transfer<D: KeyValueDB>(
    request: Json<TransferRequest>,
    _state: Data<State<D>>,
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

    let relayer_url = custody.read().await.settings.relayer_url.clone();
    // let account = custody.move_account(account_id).unwrap();

    // custody.sync_account(account_id, &state)?;

    // account.sync(&state).await?;
    let request_id = request
        .request_id
        .clone()
        .unwrap_or(Uuid::new_v4().to_string());
    // if custody.get_job_id_by_request_id(&request_id)?.is_some() {
    //     return Err(CustodyServiceError::DuplicateTransactionId);
    // }

    // request.request_id = Some(request_id.clone());

    // custody.update_task_status(task.clone(), TransferStatus::New).await?;
    // task.update_status(TransferStatus::Proving).await.unwrap();

    let c = custody.read().await;
    let account = c.account(account_id)?;
    let account = account.inner.read().await;

    let fee = 100000000;
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

    ScheduledTask::save_new(custody_db, request_id.clone())?;

    tracing::info!(
        "{} request received & saved, tx created and sent to the prover queue",
        &request_id
    );
    let task = ScheduledTask {
        request_id: request_id.clone(),
        account_id,
        // request,
        job_id: None,
        endpoint: None,
        retries_left: 42,
        status: TransferStatus::Proving,
        tx_hash: None,
        failure_reason: None,
        // account:Data::new(account),
        relayer_url,
        params,
        db,
        tx,
        callback_address: request.webhook, // custody,
        callback_sender,
    };

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

pub async fn mock_callback(
    request: Json<JobShortInfo>,
) -> Result<HttpResponse, CustodyServiceError> {
    tracing::info!(
        "received callback {:#?}", request
    );
    Ok(HttpResponse::Ok().finish())
}
pub async fn transaction_status<D: KeyValueDB>(
    request: Query<TransferStatusRequest>,
    _: Data<State<D>>,
    custody: Custody,
) -> Result<HttpResponse, CustodyServiceError> {
    let custody = custody.read().await;

    let job = custody
        .db
        .get(
            CustodyDbColumn::JobsIndex.into(),
            &request.0.request_id.into_bytes(),
        )
        .map_err(|e| CustodyServiceError::DataBaseReadError)?
        .ok_or(CustodyServiceError::TransactionNotFound)?;

    let job: JobShortInfo =
        serde_json::from_slice(&job).map_err(|_| CustodyServiceError::DataBaseReadError)?;

    // let relayer_endpoint = custody.relayer_endpoint(request.0.request_id)?;
    // let transaction_status = fetch_tx_status(&relayer_endpoint).await?;

    Ok(HttpResponse::Ok().json(job))
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
