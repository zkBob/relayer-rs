use crate::{
    configuration::Settings,
    contracts::Pool,
    routes::transactions::{query, transact},
    state::{Job, State, DB, JobsDbColumn, JobStatus}, tx_checker::check_tx,
};

use actix_web::{
    dev::Server,
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use kvdb::KeyValueDB;

use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, verifier::VK};

use std::{net::TcpListener, time::{SystemTime, Duration}, thread::sleep};
use tokio::sync::mpsc::Sender;

pub struct Application<D: 'static + KeyValueDB> {
    server: Server,
    host: String,
    port: u16,
    pub state: Data<State<D>>,
}

impl<D: 'static + KeyValueDB> Application<D> {
    pub async fn build(
        configuration: Settings,
        sender: Sender<Data<Job>>,
        pending: DB<D>,
        finalized: DB<D>,
        jobs: Data<D>, //We don't realy need a mutex, since all jobs/tx are processed independently
        vk: Option<VK<Bn256>>,
    ) -> Result<Self, std::io::Error> {
        tracing::info!("using config {:#?}", configuration);
        let vk = vk.unwrap_or(configuration.application.get_tx_vk().unwrap());
        let host = configuration.application.host;
        let address = format!("{}:{}", host, configuration.application.port);
        let web3 = Data::new(configuration.web3);

        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();

        let pool = Pool::new(web3.clone()).expect("failed to instantiate pool contract");

        let state = Data::new(State {
            pending,
            finalized,
            vk,
            pool,
            jobs,
            sender: Data::new(sender),
            web3,
        });

        let server = run(listener, state.clone())?;

        Ok(Self {
            server,
            host,
            port,
            state,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_untill_stopped(self) -> Result<(), std::io::Error> {
        tracing::info!("starting webserver at http://{}:{}", self.host, self.port);
        self.server.await
    }
}

pub fn run<D: 'static + KeyValueDB>(
    listener: TcpListener,
    state: Data<State<D>>,
) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/tx", web::get().to(query))
            .route("/transact", web::post().to(transact::<D>))
            .app_data(state.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
