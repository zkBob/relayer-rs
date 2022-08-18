use libzeropool::fawkes_crypto::{engines::bn256::Fr, ff_uint::Num};
use relayer_rs::routes::transactions::Transaction;
use std::str::FromStr;

use crate::helpers::spawn_app;

#[tokio::test]
async fn check_root_init() {
    let app = spawn_app().await.unwrap();

    let root = app.finalized.lock().unwrap().get_root();
    let root_initial = Num::<Fr>::from_str(
        "11469701942666298368112882412133877458305516134926649826543144744382391691533",
    )
    .unwrap();
    assert_eq!(root, root_initial);
}

#[tokio::test]
async fn check_root_after_tx() {
    use std::fs;
    let app = spawn_app().await.unwrap();

    let client = reqwest::Client::new();

    let file = fs::File::open("tests/data/transaction.json").unwrap();
    let tx: Transaction = serde_json::from_reader(file).unwrap();

    tracing::info!("sending request {:#?}", tx);

    let handle = tokio::spawn(async move {
        let response = client
            .post(format!("{}/sendTransaction", app.address))
            .body(serde_json::to_string(&tx).unwrap())
            .header("Content-type", "application/json")
            .send()
            .await
            .expect("failed to make request");

        app
    });

    let result = handle.await.unwrap().finalized.lock().unwrap().get_root();

    assert_eq!(
        result,
        Num::<Fr>::from_str(
            "18817649824503794997349185617629551636219455360041895422820556244689601846682"
        )
        .unwrap()
    );
}
