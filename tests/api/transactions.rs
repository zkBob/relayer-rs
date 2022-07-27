use relayer_rs::routes::transactions::Transaction;

use crate::helpers::spawn_app;

#[actix_rt::test]
async fn get_transactions_works() {
    let app = spawn_app().await.unwrap();

    let client = reqwest::Client::new();

    client
        .get(format!("{}/transations", app.address))
        .send()
        .await
        .expect("failed to make request");
}
#[actix_rt::test]
async fn post_transaction_works() {
    use std::fs;

    let app = spawn_app().await.unwrap();

    let client = reqwest::Client::new();

    let file = fs::File::open("tests/data/transaction.json").unwrap();
    let tx: Transaction = serde_json::from_reader(file).unwrap();

    tracing::info!("sending request {:#?}", tx);

    let handle = tokio::spawn(async move {
        let response = client
            .post(format!("{}/transact", app.address))
            .body(serde_json::to_string(&tx).unwrap())
            .header("Content-type", "application/json")
            .send()
            .await
            .expect("failed to make request");

        response.status().as_u16()
    });

    let result = handle.await.unwrap();
    assert_eq!(result, 200 as u16);
}

#[actix_rt::test]
async fn gen_tx_and_send() {
    let test_app = spawn_app().await.unwrap();

    let tx = test_app.generator.generate_deposit().await.unwrap();

    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/transact", test_app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request");

        assert_eq!(response.status().as_u16(), 200 as u16);
}
