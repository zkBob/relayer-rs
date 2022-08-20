use std::net::TcpListener;

use actix_cors::Cors;
use actix_http::{header, StatusCode};
use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer, ResponseError,
};
use kvdb::KeyValueDB;

use crate::{
    routes::{
        info::info,
        transactions::{query, send_transaction, send_transactions}, fee::fee,
    },
    state::State,
};

pub mod info;
pub mod fee;
pub mod transactions;

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");

    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_header(header::CONTENT_TYPE)
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .route("/tx", web::get().to(query))
            .route("/info", web::get().to(info::<D>))
            .route("/fee", web::get().to(fee::<D>))
            .route("/sendTransaction", web::post().to(send_transaction::<D>))
            .route("/sendTransactions", web::post().to(send_transactions::<D>))
            .app_data(state.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}

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
