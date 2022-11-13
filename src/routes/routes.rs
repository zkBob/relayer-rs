use std::{net::TcpListener, sync::RwLock};

use actix_cors::Cors;
use actix_http::header;
use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use kvdb::KeyValueDB;

use crate::{
    custody::{routes::{account_info, list_accounts, signup, sync_account, transfer, generate_shielded_address, history, transaction_status}, service::CustodyService},
    routes::{self, wallet_screening},
    state::State,
};

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
    custody: Data<RwLock<CustodyService>>,
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
                web::post().to(wallet_screening::get_wallet_screening_result::<D>),
            )
            .route("/signup", web::post().to(signup::<D>))
            .route("/account", web::get().to(account_info::<D>))
            .route("/accounts", web::get().to(list_accounts::<D>))
            .route("/sync", web::post().to(sync_account::<D>))
            .route("/transfer", web::post().to(transfer::<D>))
            .route("/transactionStatus", web::get().to(transaction_status::<D>))
            .route("/generateAddress", web::get().to(generate_shielded_address::<D>))
            .route("/history", web::get().to(history::<D>))
            .app_data(state.clone())
            .app_data(custody.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
