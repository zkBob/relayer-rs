use actix_http::StatusCode;
use actix_web::ResponseError;

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
}

impl From<std::io::Error> for ServiceError {
    fn from(_: std::io::Error) -> Self {
        ServiceError::InternalError
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "request failed")
    }
}

impl ResponseError for ServiceError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            ServiceError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ServiceError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
