use std::{str::FromStr, time::SystemTime};

use crate::helpers::spawn_app;
use actix_web::web::Data;
use ethabi::ethereum_types::H256;
use kvdb::{DBKey, KeyValueDB};
use libzeropool::{constants::OUT, fawkes_crypto::ff_uint::NumRepr};
use relayer_rs::{
    routes::transactions::TransactionRequest,
    state::{Job, JobStatus, JobsDbColumn},
    tx_checker::{check_tx, start_poller},
};
use uuid::Uuid;

#[actix_rt::test]
async fn get_transactions_works() {
    let app = spawn_app(false).await.unwrap();

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

    let app = spawn_app(false).await.unwrap();

    let client = reqwest::Client::new();

    let file = fs::File::open("tests/data/transaction.json").unwrap();
    let tx: TransactionRequest = serde_json::from_reader(file).unwrap();

    tracing::info!("sending request {:#?}", tx);

    let result = client
        .post(format!("{}/transact", app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request")
        .status()
        .as_u16();
    assert_eq!(result, 200 as u16);
}

#[actix_rt::test]
async fn gen_tx_and_send() {
    let test_app = spawn_app(true).await.unwrap();

    let generator = test_app.generator.unwrap();

    let request_id = uuid::Uuid::new_v4();

    let tx = generator.generate_deposit(Some(request_id)).await.unwrap();

    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/transact", test_app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request");

    assert_eq!(response.status().as_u16(), 200 as u16);

    assert!(test_app
        .state
        .jobs
        .has_key(0, request_id.as_bytes())
        .unwrap());

    let pending = test_app.state.pending.lock().unwrap();
    {
        let next_index = pending.next_index();
        assert_eq!(next_index, OUT as u64 + 1);
    }

    let finalized = test_app.state.finalized.lock().unwrap();
    {
        assert_eq!(finalized.next_index(), 0 as u64);
    }
}

#[actix_rt::test]
async fn test_parse_fee_from_tx() {
    // let tx_as_str = &std::fs::read_to_string("./tests/data/deposit.json").unwrap();
    // println!("tx as str {}", tx_as_str);
    let app = spawn_app(true).await.unwrap();
    let generator = app.generator.expect("need generator to generate tx");

    let tx_request = generator.generate_deposit(None).await.unwrap();

    let memo = memo_parser::memo::Memo::parse_memoblock(
        &tx_request.memo.bytes().collect(),
        memo_parser::memo::TxType::DepositPermittable,
    );

    println!("memo fee: {:#?} ", memo.fee);
}

#[actix_rt::test]
async fn test_finalize() {
    let app = spawn_app(false).await.unwrap();

    app.state.sync().await.unwrap();

    for (key, job_as_bytes) in app.state.jobs.iter(JobsDbColumn::Jobs as u32) {


            let job:Job = serde_json::from_slice(&job_as_bytes).unwrap();
            tracing::info!("retrieved {} \t {:#?}", hex::encode(key), job.transaction.unwrap().hash);
    }

    let tx_hash_as_bytes =
        &hex::decode("2b1673b1759f7db0480273a6360dee57668ff301c578bc3d5843271d1c818ac7").unwrap()[..];
        
    let job_as_bytes = app
        .state
        .jobs
        .get(JobsDbColumn::Jobs as u32, tx_hash_as_bytes)
        .unwrap()
        .unwrap();

    let mut job: Job = serde_json::from_slice(&job_as_bytes).unwrap();

    job.status = JobStatus::Mining;

    app.state.jobs.transaction().put(
        JobsDbColumn::Jobs as u32,
        tx_hash_as_bytes,
        serde_json::to_string(&job).unwrap().as_bytes(),
    );

    check_tx(job, app.state).await.unwrap();
}
