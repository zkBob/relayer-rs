use std::sync::Arc;

use actix_web::web::Data;
use libzeropool::fawkes_crypto::backend::bellman_groth16::verifier;
use once_cell::sync::Lazy;
use relayer_rs::configuration::{get_config, Settings};
use relayer_rs::routes::transactions::Transaction;
use relayer_rs::startup::Application;
use relayer_rs::telemetry::{get_subscriber, init_subscriber};
use tokio::sync::mpsc;
pub struct TestApp {
    pub address: String,
    pub port: u16,
}
static TRACING: Lazy<()> = Lazy::new(|| {
    if std::env::var("TEST_LOG").is_ok() {
        init_subscriber(get_subscriber(
            "test".into(),
            "info".into(),
            std::io::stdout,
        ))
    } else {
        init_subscriber(get_subscriber("test".into(), "info".into(), std::io::sink))
    }
});

pub async fn spawn_app() -> Result<TestApp, std::io::Error> {
    Lazy::force(&TRACING);
    let config: Settings = {
        let mut c = get_config().expect("failed to get config");
        c.application.port = 0;
        c
    };

    let (sender, mut rx) = mpsc::channel::<Arc<Transaction>>(1000);

    let app = Application::build(config.clone(), sender).await?;

    let port = app.port();

    
    
    let address = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            tracing::info!("Received tx {:#?}", tx.memo);
        }
    });

    tokio::spawn(app.run_untill_stopped());

    Ok(TestApp { address, port })
}
