use crate::{
    configuration::Settings,
    routes::transactions::{query, transact, Transaction},
};
use actix_web::{dev::Server, middleware, web, App, HttpServer};

use std::net::TcpListener;
use tokio::sync::mpsc::Sender;
pub struct Application {
    server: Server,
    port: u16,
    // rx: Receiver<Transaction>,
}

impl Application {
    pub async fn build(
        configuration: Settings,
        sender: Sender<Transaction>,
    ) -> Result<Self, std::io::Error> {
        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );

        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();
        // let (sender, rx) = mpsc::channel::<Transaction>(1000);
        let server = run(listener, sender)?;

        Ok(Self {
            server,
            port,
            // rx,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_untill_stopped(self) -> Result<(), std::io::Error> {
        tracing::info!("starting webserver");
        self.server.await
    }

}

pub fn run(listener: TcpListener, sender: Sender<Transaction>) -> Result<Server, std::io::Error> {
    tracing::info!("starting webserver");
    let sender = web::Data::new(sender);
    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/tx", web::get().to(query))
            .route("/transact", web::post().to(transact))
            .app_data(sender.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}
