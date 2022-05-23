use crate::{
    configuration::Settings,
    routes::transactions::{query, transact, Transaction},
};
use actix_web::{dev::Server, middleware, web, App, HttpServer};

use std::net::TcpListener;
use tokio::sync::mpsc::Sender;

pub struct Application {
    server: Server,
    host: String,
    port: u16,
    
    // rx: Receiver<Transaction>,
}

impl Application {
    pub async fn build(
        configuration: Settings,
        sender: Sender<Transaction>,
    ) -> Result<Self, std::io::Error> {
        let tx_vk = configuration.application.get_tx_vk().unwrap();
        let host = configuration.application.host;
        let address = format!("{}:{}", host, configuration.application.port);

        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();
        let server = run(listener, sender , configuration.application.tx.vk)?;
        
        Ok(Self { server, host, port})
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_untill_stopped(self) -> Result<(), std::io::Error> {
        tracing::info!("starting webserver at http://{}:{}", self.host, self.port);
        self.server.await
    }
}

pub fn run(listener: TcpListener, sender: Sender<Transaction> , vk_path: String) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");
    let sender = web::Data::new(sender);
    
    


    // let handle = tokio::spawn(async {

    //     let vk = web::Data::new(tx_vk).clone();
    //     move || {
    //         transact(request, sender, vk);
    //     }
    // });
    // verifier::verify(&tx_vk, &tx.proof.proof, &tx.proof.inputs);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/tx", web::get().to(query))
            .route("/transact", web::post().to(transact))
            .app_data(sender.clone())
            .app_data(web::Data::new(vk_path))
    })
    .listen(listener)?
    .run();
    Ok(server)
}
