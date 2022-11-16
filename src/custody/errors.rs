use actix_http::StatusCode;
use actix_web::{http::header::ContentType, HttpResponse, ResponseError};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CustodyServiceError {
    #[error("request malformed")]
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
}


impl ResponseError for CustodyServiceError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            CustodyServiceError::TransactionNotFound
            | CustodyServiceError::DuplicateTransactionId
            | CustodyServiceError::BadRequest(_)
            | CustodyServiceError::IncorrectAccountId
            | CustodyServiceError::AccountNotFound => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        #[derive(Serialize)]
        struct ErrorResponse {
            success: bool,
            error: String,
        }

        let response = serde_json::to_string(&ErrorResponse {
            success: false,
            error: format!("{}", self),
        })
        .unwrap();

        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(response)
    }
}
