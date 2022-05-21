

use std::thread;

use relayer_rs::routes::transactions::Transaction;

use crate::helpers::spawn_app;



#[actix_rt::test]
async fn get_transactions_works() {
    let app = spawn_app().await;

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
    // use std::io::{prelude::*, BufReader};
    let app = spawn_app().await;

    let client = reqwest::Client::new();    
    
    // let tx = fs::read_to_string("tests/data/transaction.json").unwrap();
    // let content = fs::read("tests/data/transaction.json").unwrap();
    let file =fs::File::open("tests/data/transaction.json").unwrap();
    let tx : Transaction= serde_json::from_reader(file).unwrap();

    
    tracing::info!("sending request {:#?}", tx);

    let response = client
        .post(format!("{}/transact", app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request");

    assert_eq!(response.status().as_u16(), 200 as u16);
    
}
