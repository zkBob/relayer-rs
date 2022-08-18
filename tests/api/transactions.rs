use libzeropool::fawkes_crypto::ff_uint::Num;
use crate::helpers::spawn_app;
use kvdb::{DBKey, DBOp, DBTransaction, KeyValueDB};
use libzeropool::constants::OUT;
use relayer_rs::{
    routes::transactions::TransactionRequest,
    state::{Job, JobStatus, JobsDbColumn},
    tx_checker::check_tx,
};

use serde_json::{json, Value};
use web3::types::BlockNumber;
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, ResponseTemplate};

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
        .post(format!("{}/sendTransaction", app.address))
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
        .post(format!("{}/sendTransaction", test_app.address))
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
async fn test_check_pending_state_after_tx() {
    let test_app = spawn_app(true).await.unwrap();
    {
        let pending_state = test_app.state.pending.lock().unwrap();
        let root = pending_state.get_root();
        assert_eq!(root, Num::from("11469701942666298368112882412133877458305516134926649826543144744382391691533"));
    }
    use std::fs;
    let file = fs::File::open("tests/data/transaction.json").unwrap();
    let tx: TransactionRequest = serde_json::from_reader(file).unwrap();
    let client = reqwest::Client::new();
    client
        .post(format!("{}/transact", test_app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request");
    {
        let pending_state = test_app.state.pending.lock().unwrap();
        let root = pending_state.get_root();
        assert_eq!(root, Num::from("16008527574580846904606969406329027992616751511283117704657085855575006190079"));
    }
}

#[actix_rt::test]
async fn test_check_pending_state_after_two_tx() {
    let test_app = spawn_app(true).await.unwrap();
    {
        let pending_state = test_app.state.pending.lock().unwrap();
        let root = pending_state.get_root();
        assert_eq!(root, Num::from("11469701942666298368112882412133877458305516134926649826543144744382391691533"));
    }
    use std::fs;
    let file = fs::File::open("tests/data/transaction.json").unwrap();
    let tx: TransactionRequest = serde_json::from_reader(file).unwrap();
    let client = reqwest::Client::new();
    client
        .post(format!("{}/transact", test_app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request");
    {
        let pending_state = test_app.state.pending.lock().unwrap();
        let root = pending_state.get_root();
        assert_eq!(root, Num::from("16008527574580846904606969406329027992616751511283117704657085855575006190079"));
    }
    client
        .post(format!("{}/transact", test_app.address))
        .body(serde_json::to_string(&tx).unwrap())
        .header("Content-type", "application/json")
        .send()
        .await
        .expect("failed to make request");
    {
        let pending_state = test_app.state.pending.lock().unwrap();
        let root = pending_state.get_root();
        assert_eq!(root, Num::from("18654607918309982490299585095288248711247171096327425318142876620343469986659"));
    }
}

#[actix_rt::test]
async fn test_finalize() {
    let app = spawn_app(false).await.unwrap();

    app.state.sync().await.unwrap();

    for (key, job_as_bytes) in app.state.jobs.iter(JobsDbColumn::Jobs as u32) {
        let job: Job = serde_json::from_slice(&job_as_bytes).unwrap();
        tracing::info!(
            "retrieved {} \t {:#?}",
            hex::encode(key),
            job.transaction.unwrap().hash
        );
    }

    let (id, job_as_bytes) = app
        .state
        .jobs
        .iter(JobsDbColumn::Jobs as u32)
        .next()
        .unwrap();

    let mut job: Job = serde_json::from_slice(&job_as_bytes).unwrap();

    job.status = JobStatus::Mining;

    app.state.jobs.transaction().put(
        JobsDbColumn::Jobs as u32,
        &id,
        serde_json::to_string(&job).unwrap().as_bytes(),
    );

    let state = app.state.clone();

    check_tx(job, state).await.unwrap();

    let state = app.state.clone();

    let updated_job: Job = app
        .state
        .jobs
        .get(JobsDbColumn::Jobs as u32, &id)
        .unwrap()
        .map(|v| serde_json::from_slice(&v).unwrap())
        .unwrap();

    assert_eq!(updated_job.status, JobStatus::Done);

    let finalized = state.finalized.lock().unwrap();
    let pending = state.pending.lock().unwrap();
    assert_eq!(finalized.next_index(), pending.next_index());
}


#[actix_rt::test]
async fn test_rollback() {
    /*
    1. Sync state, pulling to 2 transactions from contract
    2. Query last job from Jobs DB, update it's status to "Mining" and trigger the check_tx (instead of scheduler)
    3. Check_tx is supposed to get a modified tx receipt from mock with "revert"
    4. Check_tx is expected to:
        a. rollback pending state to finalized state
        b. mark Job as rejected in the database
    */
    let app = spawn_app(false).await.unwrap();

    /*
    Mock two successful events
     */
    let _mock = Mock::given(method("POST"))
        .and(body_string_contains("eth_getLogs"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(std::fs::read_to_string("tests/data/logs.json").unwrap()),
        )
        .mount_as_scoped(&app.mock_server)
        .await;

    let eth_call_response = json!({"id":0,"jsonrpc":"2.0","result":"0x0000000000000000000000000000000000000000000000000000000000000100"});

    /*
    Mock current pool contract root
     */
    Mock::given(method("POST"))
        .and(body_string_contains("eth_call"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&eth_call_response))
        .mount(&app.mock_server)
        .await;

    /*
    Mock both transaction content and receipt to sync DB
     */
    Mock::given(method("POST"))
        .and(body_string_contains("eth_getTransactionByHash"))
        .and(body_string_contains(
            "0x2b1673b1759f7db0480273a6360dee57668ff301c578bc3d5843271d1c818ac7",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(std::fs::read_to_string("tests/data/0x2b1673b1759f7db0480273a6360dee57668ff301c578bc3d5843271d1c818ac7.json").unwrap()),
        )
        .mount(&app.mock_server)
        .await;

    Mock::given(method("POST"))
        .and(body_string_contains("eth_getTransactionReceipt"))
        .and(body_string_contains(
            "0x2b1673b1759f7db0480273a6360dee57668ff301c578bc3d5843271d1c818ac7",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(std::fs::read_to_string("tests/data/r_0x2b1673b1759f7db0480273a6360dee57668ff301c578bc3d5843271d1c818ac7.json").unwrap()),
        )
        .mount(&app.mock_server)
        .await;

    Mock::given(method("POST"))
        .and(body_string_contains("eth_getTransactionReceipt"))
        .and(body_string_contains(
            "0x62a658acb631e785bb4a494781f6411a84bceee685112fbceb3601ded279f6ac",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(std::fs::read_to_string("tests/data/r_0x62a658acb631e785bb4a494781f6411a84bceee685112fbceb3601ded279f6ac.json").unwrap()),
        )
        .mount(&app.mock_server)
        .await;

    Mock::given(method("POST"))
        .and(body_string_contains("eth_getTransactionByHash"))
        .and(body_string_contains(
            "0x62a658acb631e785bb4a494781f6411a84bceee685112fbceb3601ded279f6ac",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(std::fs::read_to_string("tests/data/0x62a658acb631e785bb4a494781f6411a84bceee685112fbceb3601ded279f6ac.json").unwrap()),
        )
        .mount(&app.mock_server)
        .await;

    let state = app.state.clone();

    match app.state.sync().await {
        Ok(_) => tracing::debug!("sync complete"),
        Err(e) => {
            tracing::error!("error whilst sync {:?}", e);
            for request in app
                .mock_server
                .received_requests()
                .await
                .unwrap()
                .into_iter()
            {
                tracing::debug!(
                    "request received {:#?} ",
                    String::from_utf8(request.body).unwrap()
                );
            }
        }
    }

    // Rollback finalized state manually to simulate a gap between pending and finalized state
    {
        let mut finalized = app.state.finalized.lock().unwrap();

        finalized.rollback(0 as u64);
    }

    // Mutate received jobs to simulate processing, bot jobs get mining state and a corresponding key among scheduled checks (TxCheckTasks) 
    for (id, job_as_bytes) in app.state.jobs.iter(JobsDbColumn::Jobs as u32) {

        let mut job: Job = serde_json::from_slice(&job_as_bytes).unwrap();

        job.status = JobStatus::Mining;

        let key = DBKey::from_slice(&id);

        app.state
            .jobs
            .write(DBTransaction {
                ops: vec![
                    DBOp::Insert {
                        col: JobsDbColumn::Jobs as u32,
                        key: DBKey::from_slice(&id),
                        value: serde_json::to_string(&job).unwrap().as_bytes().to_vec(),
                    },
                    DBOp::Insert {
                        col: JobsDbColumn::TxCheckTasks as u32,
                        key,
                        value: vec![],
                    },
                ],
            })
            .unwrap();
    }

    let (_id, last_job) = app
        .state
        .jobs
        .iter(JobsDbColumn::Jobs as u32)
        .next()
        .unwrap();

    let last_job: Job = serde_json::from_slice(&last_job).unwrap();

    {
        let pending = app.state.pending.lock().unwrap();

        assert_eq!(pending.next_index(), 256 as u64, "wrong index after sync");
    }

    {
        let finalized = app.state.finalized.lock().unwrap();

        assert_eq!(finalized.next_index(), 0 as u64, "finalized db must be at vanilla state");
    }

    assert!(
        app.state
            .jobs
            .iter(JobsDbColumn::Jobs as u32)
            .all(|(_, v)| {
                let job: Job = serde_json::from_slice(&v).unwrap();
                job.status == JobStatus::Mining
            }),
        "all job must have been set to Mining status "
    );


    match check_tx(last_job, state).await {
        Ok(status) => tracing::info!("check_tx result: {:#?}", status),
        Err(e) => {
            tracing::error!("error whilst check_tx {:?}", e);
            for request in app
                .mock_server
                .received_requests()
                .await
                .unwrap()
                .into_iter()
            {
                tracing::debug!(
                    "request received {:#?} ",
                    String::from_utf8(request.body).unwrap()
                );
            }
        }
    }

    {
        let pending = app.state.pending.lock().unwrap();

        assert_eq!(pending.next_index(), 0 as u64);
    }

    {
        let finalized = app.state.finalized.lock().unwrap();

        assert_eq!(finalized.next_index(), 0 as u64);
    }

    assert!(
        app.state
            .jobs
            .iter(JobsDbColumn::Jobs as u32)
            .all(|(_, v)| {
                let job: Job = serde_json::from_slice(&v).unwrap();
                job.status == JobStatus::Rejected
            }),
        "all job must have been rejected "
    );
}

#[actix_rt::test]
pub async fn test_get_logs() {
    let app = spawn_app(false).await.unwrap();

    let body = std::fs::read_to_string("tests/data/logs.json").unwrap();

    // let body: Vec<web3::types::Log> = serde_json::from_slice(&body[..]).unwrap();
    let _mock = Mock::given(method("POST"))
        // .and(path("/eth_getLogs"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount_as_scoped(&app.mock_server)
        .await;

    let logs = app.state.pool.get_logs().await.unwrap();

    tracing::debug!("got logs {:#?}", logs);
}

#[actix_rt::test]
pub async fn test_get_events() {
    let app = spawn_app(false).await.unwrap();

    let body = std::fs::read_to_string("tests/data/logs.json").unwrap();

    // let body: Vec<web3::types::Log> = serde_json::from_slice(&body[..]).unwrap();
    let _mock = Mock::given(method("POST"))
        // .and(path("/eth_getLogs"))
        .and(body_string_contains("eth_getLogs"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount_as_scoped(&app.mock_server)
        .await;

    let eth_call_response = json!({"id":0,"jsonrpc":"2.0","result":"0x0000000000000000000000000000000000000000000000000000000000000100"});

    Mock::given(method("POST"))
        .and(body_string_contains("eth_call"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&eth_call_response))
        .mount(&app.mock_server)
        .await;

    let logs = app
        .state
        .pool
        .get_events(Some(BlockNumber::Earliest), Some(BlockNumber::Latest), None)
        .await
        .unwrap();

    tracing::debug!("got logs {:#?}", logs);
}

#[actix_rt::test]
pub async fn test_logs_mock() {
    let app = spawn_app(false).await.unwrap();

    let body = std::fs::read_to_string("tests/data/logs.json").unwrap();

    // let body: Vec<web3::types::Log> = serde_json::from_slice(&body[..]).unwrap();
    let _mock = Mock::given(method("POST"))
        .and(body_string_contains("eth_getLogs"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&app.mock_server)
        .await;

    let eth_call_response = json!({"id":0,"jsonrpc":"2.0","result":"0x0000000000000000000000000000000000000000000000000000000000000100"});
    Mock::given(method("POST"))
        .and(body_string_contains("eth_call"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&eth_call_response))
        .mount(&app.mock_server)
        .await;

    let logs: Value = reqwest::Client::new()
        .post("http://localhost:8546")
        .json(&serde_json::json!(
            {
                "jsonrpc": "2.0",
                "method": "eth_getLogs",
                "params": [
                    {
                        "address": "0xc89ce4735882c9f0f0fe26686c53074e09b0d550",
                        "fromBlock": "earliest",
                        "toBlock": "latest",
                        "topics": [
                            "0x7d39f8a6bc8929456fba511441be7361aa014ac6f8e21b99990ce9e1c7373536"
                        ]
                    }
                ],
                "id": 42
            }
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    tracing::debug!("got logs {:#?}", logs);
}
