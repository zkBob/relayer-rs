use actix_http::StatusCode;
use actix_web::{ResponseError, http::header::ContentType, HttpResponse};
use serde::Serialize;

use crate::routes::{
    fee::fee,
    info::info,
    job::job,
    send_transactions::{query, send_transaction, send_transactions},
    transactions::transactions,
};

mod fee;
mod info;
mod job;
pub mod routes;
pub mod send_transactions;
mod transactions;
pub mod wallet_screening;

#[derive(Debug)]
pub enum ServiceError {
    BadRequest(String),
    InternalError,
    AccountNotFound
}

impl From<std::io::Error> for ServiceError {
    fn from(_: std::io::Error) -> Self {
        ServiceError::InternalError
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let error = match self {
            ServiceError::BadRequest(err) => format!("BadRequest: {}", err),
            ServiceError::InternalError => format!("InternalError"),
            ServiceError::AccountNotFound => "Account not found".to_string(),
        };

        write!(f, "{}", error)
    }
}

impl ResponseError for ServiceError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            ServiceError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ServiceError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            ServiceError::AccountNotFound => StatusCode::NOT_FOUND,
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
