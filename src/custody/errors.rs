use actix_http::StatusCode;
use actix_web::{ResponseError, HttpResponse, http::header::ContentType};
use serde::Serialize;

#[derive(Debug)]
pub enum CustodyServiceError {
    BadRequest(String),
    CustodyLockError,
    StateSyncError,
    IncorrectAccountId,
    AccountNotFound,
    DuplicateTransactionId,
    DataBaseReadError,
    DataBaseWriteError,
    RelayerSendError,
    TransactionNotFound,
}

// impl From<std::io::Error> for ServiceError {
//     fn from(_: std::io::Error) -> Self {
//         ServiceError::InternalError
//     }
// }

impl std::fmt::Display for CustodyServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let error = match self {
            CustodyServiceError::BadRequest(err) => format!("BadRequest: {}", err),
            CustodyServiceError::CustodyLockError => format!("InternalError: failed to lock custody service"),
            CustodyServiceError::StateSyncError => format!("InternalError: failed to sync custody state"),
            CustodyServiceError::IncorrectAccountId => format!("BadRequest: incorrect account id"),
            CustodyServiceError::AccountNotFound => format!("BadRequest: account with such id not found"),
            CustodyServiceError::DuplicateTransactionId => format!("BadRequest: transaction with such id already exists"),
            CustodyServiceError::DataBaseReadError => format!("InternalError: failed to read from database"),
            CustodyServiceError::DataBaseWriteError => format!("InternalError: failed to write to database"),
            CustodyServiceError::RelayerSendError => format!("InternalError: failed to send request to relayer"),
            CustodyServiceError::TransactionNotFound => format!("BadRequest: transaction with such id not found"),
        };

        write!(f, "{}", error)
    }
}

impl ResponseError for CustodyServiceError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            CustodyServiceError::BadRequest(_) => StatusCode::BAD_REQUEST,
            CustodyServiceError::CustodyLockError => StatusCode::INTERNAL_SERVER_ERROR,
            CustodyServiceError::StateSyncError => StatusCode::INTERNAL_SERVER_ERROR,
            CustodyServiceError::IncorrectAccountId => StatusCode::BAD_REQUEST,
            CustodyServiceError::AccountNotFound => StatusCode::BAD_REQUEST,
            CustodyServiceError::DuplicateTransactionId => StatusCode::BAD_REQUEST,
            CustodyServiceError::DataBaseReadError => StatusCode::INTERNAL_SERVER_ERROR,
            CustodyServiceError::DataBaseWriteError => StatusCode::INTERNAL_SERVER_ERROR,
            CustodyServiceError::RelayerSendError => StatusCode::INTERNAL_SERVER_ERROR,
            CustodyServiceError::TransactionNotFound => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        #[derive(Serialize)]
        struct ErrorResponse {
            success: bool,
            error: String,
        }
        
        let response = serde_json::to_string(&ErrorResponse{
            success: false,
            error: format!("{}", self),
        }).unwrap();

        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(response)
    }
}