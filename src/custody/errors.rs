use actix_http::StatusCode;
use actix_web::{http::header::ContentType, HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Serialize, Deserialize, Debug, Error, PartialEq)]
pub enum CustodyServiceError {
    #[error("request malformed or invalid: {0}")]
    BadRequest(String),
    #[error("internal error")]
    CustodyLockError,
    #[error("internal error")]
    StateSyncError,
    #[error("bad account id")]
    IncorrectAccountId,
    #[error("bad account id")]
    AccountNotFound,
    #[error("request id already exists")]
    DuplicateTransactionId,
    #[error("internal error")]
    DataBaseReadError,
    #[error("internal error")]
    DataBaseWriteError(String),
    #[error("internal error")]
    RelayerSendError,
    #[error("request not found")]
    TransactionNotFound,
    #[error("general error occured:'{0}'")]
    InternalError(String),
    #[error("retries exhausted")]
    RetriesExhausted,
    #[error("relayer returned error: '{0}'")]
    TaskRejectedByRelayer(String),
    #[error("need retry")]
    RetryNeeded,
    #[error("access denied")]
    AccessDenied,
    #[error("previous tx failed")]
    PreviousTxFailed,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("account is busy")]
    AccountIsBusy,
    #[error("account is not synced yet")]
    AccountIsNotSynced,
    #[error("service is busy")]
    ServiceIsBusy,
}

impl ResponseError for CustodyServiceError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            CustodyServiceError::TransactionNotFound
            | CustodyServiceError::DuplicateTransactionId
            | CustodyServiceError::BadRequest(_)
            | CustodyServiceError::IncorrectAccountId
            | CustodyServiceError::AccountNotFound => StatusCode::BAD_REQUEST,
            CustodyServiceError::AccessDenied => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        #[derive(Serialize)]
        struct ErrorResponse {
            error: String,
        }

        let response = serde_json::to_string(&ErrorResponse {
            error: format!("{}", self),
        })
        .unwrap();

        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(response)
    }
}
