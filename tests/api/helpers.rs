use relayer_rs::configuration::{get_config, Settings};
use relayer_rs::routes::transactions::Transaction;
use relayer_rs::startup::Application;
use relayer_rs::telemetry::{init_subscriber, get_subscriber};
use tokio::sync::mpsc;
use once_cell::sync::Lazy;
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

pub async fn spawn_app() -> TestApp {
    Lazy::force(&TRACING);
    let config: Settings = {
        let mut c = get_config().expect("failed to get config");
        c.application.port = 0;
        c
    };

    let (sender, mut rx) = mpsc::channel::<Transaction>(1000);

    let app = Application::build(config.clone(),sender).await.unwrap();

    let port = app.port();


    let address = format!("http://127.0.0.1:{}", port);

    // let port = app.port();

    tokio::spawn( async move {
        tracing::info!("starting receiver");
        while let Some(tx) = rx.recv().await {
            tracing::info!("Received tx {:#?}", tx.memo);
        }
    });
    
    tokio::spawn(app.run_untill_stopped());

    

    TestApp { address, port }
}
