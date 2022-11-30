use std::{net::TcpListener,};

use actix_cors::Cors;
use actix_http::header;
use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer, HttpResponse,
};
use kvdb::KeyValueDB;
use kvdb_rocksdb::Database;
use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, Parameters};
use tokio::sync::{mpsc::Sender, RwLock};

use crate::{
    custody::{routes::{account_info, list_accounts, signup, transfer, generate_shielded_address, history, transaction_status, calculate_fee, update_start_block}, service::{CustodyService, JobStatusCallback}, scheduled_task::ScheduledTask},
    routes::{self, wallet_screening},
    state::State,
};
// use opentelemetry::{
//     sdk::export::trace::
// };
use actix_web_opentelemetry::RequestTracing;


pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
    custody: Data<RwLock<CustodyService>>,
    params: Data<Parameters<Bn256>>,
    custody_db: Data<Database>,
    prover_queue: Data<Sender<ScheduledTask<D>>>,
    callback_queue: Data<Sender<JobStatusCallback>>
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
            .wrap(RequestTracing::new())
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(|| HttpResponse::Ok()))
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
            .route("/transfer", web::post().to(transfer::<D>))
            .route("/calculateFee", web::get().to(calculate_fee::<D>))
            .route("/transactionStatus", web::get().to(transaction_status::<D>))
            .route("/generateAddress", web::get().to(generate_shielded_address::<D>))
            .route("/history", web::get().to(history::<D>))
            .route("/callback_mock", web::post().to(crate::custody::routes::callback_mock))
            .route("/updateStartBlock", web::post().to(update_start_block::<D>))
            .app_data(state.clone())
            .app_data(custody.clone())
            .app_data(params.clone())
            .app_data(custody_db.clone())
            .app_data(prover_queue.clone())
            .app_data(callback_queue.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
