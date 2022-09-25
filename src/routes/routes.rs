use std::net::TcpListener;

use actix_cors::Cors;
use actix_http::header;
use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use kvdb::KeyValueDB;
// use opentelemetry::{
//     sdk::export::trace::
// };
use opentelemetry::{global, sdk::propagation::TraceContextPropagator};
use opentelemetry::trace::Tracer;
use tracing_subscriber::prelude::*;
use opentelemetry_jaeger::new_agent_pipeline;
use tracing_subscriber::Registry;
use actix_web_opentelemetry::RequestTracing;
use crate::{routes, state::State};

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");

    let server = HttpServer::new(move || {

        global::set_text_map_propagator(TraceContextPropagator::new());

        let tracer = new_agent_pipeline()
            .with_service_name("app_name")
            //.install_batch TODO: requires passing a runtime all the way from entry point
            .install_simple()
            .unwrap();
            
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_header(header::CONTENT_TYPE)
            .max_age(3600);

            Registry::default()
            .with(tracing_subscriber::EnvFilter::new("INFO"))
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .init();
            
        App::new()
            .wrap(cors)
            .wrap(RequestTracing::new())
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
            .app_data(state.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
