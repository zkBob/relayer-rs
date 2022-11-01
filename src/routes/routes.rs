use std::{net::TcpListener, sync::Mutex};

use actix_cors::Cors;
use actix_http::header;
use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use kvdb::KeyValueDB;

use crate::{routes::{self, wallet_screening}, state::State, custody::service::{signup, CustodyService}};

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
    custody: Data<Mutex<CustodyService>>
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
            .route("/tx", web::get().to(routes::query))
            .route("/info", web::get().to(routes::info::<D>))
            .route("/fee", web::get().to(routes::fee::<D>))
            .route("/job/{job_id}", web::get().to(routes::job::<D>))
            .route("/transactions/v2", web::get().to(routes::transactions::<D>))
            .route(
                "/sendTransaction",
                web::post().to(routes::send_transaction::<D>),
            )
            .route(
                "/sendTransactions",
                web::post().to(routes::send_transactions::<D>),
            )
            .route(
                "/wallet_screening",
                web::post().to(wallet_screening::get_wallet_screening_result::<D>)
            )
            .route("/signup", web::post().to(signup))
            .app_data(state.clone())
            .app_data(custody.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
