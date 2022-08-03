use libzeropool::fawkes_crypto::ff_uint::Num;
use relayer_rs::routes::transactions::TransactionRequest;

use crate::helpers::spawn_app;

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

    let tx = generator.generate_deposit().await.unwrap();

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

#[actix_rt::test]
async fn test_parse_fee_from_tx() {
    // let tx_as_str = &std::fs::read_to_string("./tests/data/deposit.json").unwrap();
    // println!("tx as str {}", tx_as_str);
    let app = spawn_app(true).await.unwrap();
    let generator = app.generator.expect("need generator to generate tx");

    let tx_request = generator.generate_deposit().await.unwrap();
    

    let memo = memo_parser::memo::Memo::parse_memoblock(
        tx_request.memo.bytes().collect(),
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
