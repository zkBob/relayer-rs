use crate::{
    configuration::Settings,
    contracts::Pool,
    custody::{service::CustodyService, types::ScheduledTask},
    routes::routes,
    state::{State, DB},
    types::job::Job,
};

use actix_web::{dev::Server, web::Data};
use kvdb::KeyValueDB;

use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, verifier::VK};

use std::{net::TcpListener, sync::RwLock};
use tokio::sync::mpsc::{self, Sender};

pub struct Application<D: 'static + KeyValueDB> {
    server: Server,
    host: String,
    port: u16,
    pub state: Data<State<D>>,
    // pub custody: Data<Mutex<CustodyService>>
}

impl<D: 'static + KeyValueDB> Application<D> {
    pub async fn build(
        configuration: Settings,
        sender: Sender<Job>,
        pending: DB<D>,
        finalized: DB<D>,
        jobs: Data<D>, //We don't realy need a mutex, since all jobs/tx are processed independently
        vk: Option<VK<Bn256>>,
    ) -> Result<Self, std::io::Error> {
        tracing::info!("using config {:#?}", configuration);
        let vk = vk.unwrap_or(configuration.application.get_tx_vk().unwrap());
        let pool = Pool::new(&configuration.web3).expect("failed to instantiate pool contract");

        let state = Data::new(State {
            pending,
            finalized,
            vk,
            pool,
            jobs,
            sender: Data::new(sender),
            settings: Data::new(configuration.clone()),
        });

        let tx_params = Data::new(configuration.application.get_tx_params());
        let host = configuration.application.host;
        let address = format!("{}:{}", host, configuration.application.port);
        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();
        let db = Data::new(CustodyService::get_db(&configuration.custody.db_path));

        let (prover_sender, prover_receiver) = mpsc::channel::<ScheduledTask>(100);
        let (status_sender, status_receiver) = mpsc::channel::<ScheduledTask>(100);
        let (webhook_sender, webhook_receiver) = mpsc::channel::<ScheduledTask>(100);

        let custody = Data::new(RwLock::new(CustodyService::new(
            // tx_params,
            configuration.custody,
            state.clone(),
            db.clone(),
        )));

        let server = routes::run(
            listener,
            state.clone(),
            custody,
            tx_params,
            db,
            Data::new(prover_sender),
        )?;
        // let custody = custody.clone();
        Ok(Self {
            server,
            host,
            port,
            state,
            // custody
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
