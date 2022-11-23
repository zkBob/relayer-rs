use std::time::Duration;

use relayer_rs::custody::{
    account::Account as CustodyAccount,
    routes::transfer,
    types::{TransferRequest, TransferResponse},
};
use tokio::time::sleep;
use uuid::Uuid;
use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};

use crate::api::helpers::spawn_app;
use kvdb_rocksdb::Database;
use libzeropool::helpers::sample_data::State as SampleState;
use libzeropool::{
    fawkes_crypto::rand::{thread_rng, Rng},
    native::account::Account,
};
use libzeropool::{native::params::PoolBN256, POOL_PARAMS};
use libzkbob_rs::client::{state::State, UserAccount};
#[actix_rt::test]
pub async fn test_callback() {
    let app = spawn_app(false).await.unwrap();

    // let _mock = Mock::given(method("POST"))
    //     .and(path("/callback"))
    //     .respond_with(ResponseTemplate::new(200))
    //     .mount_as_scoped(&app.mock_server)
    //     .await;

    // let mut rng = thread_rng();

    // let sample_state = SampleState::sample_deterministic_state(&mut rng, &*POOL_PARAMS, 42 as u64);

    // let mut zp_state: State<kvdb_memorydb::InMemory, PoolBN256> =
    //     State::init_test(POOL_PARAMS.clone());

    // for (index, (account, _)) in sample_state.items.into_iter().enumerate() {
    //     zp_state.add_account(index as u64, account);
    // }

    // let user_account = UserAccount::new(sample_state.sigma, zp_state, POOL_PARAMS.clone());

    // let account_id = Uuid::new_v4();

//     let acc = CustodyAccount::new_test(account_id, Some(user_account)).unwrap();

//     {
//         let mut custody = app.custody.write().await;

//     custody.accounts.push(acc);
// }

//     let custody = app.custody.read().await;
//     let address = custody.gen_address(account_id).await.unwrap();

//     let request_id = Uuid::new_v4();

//     let transfer_request = TransferRequest {
//         request_id: Some(request_id.to_string()),
//         account_id: account_id.to_string(),
//         amount: 42,
//         to: address,
//         webhook: None,
//     };

    // let transfer_response: TransferResponse = reqwest::Client::new()
    //     .post("http://localhost:8001/transfer")
    //     .json(&serde_json::to_value(transfer_request).unwrap())
    //     .send()
    //     .await
    //     .unwrap()
    //     .json()
    //     .await
    //     .unwrap();

    // tracing::info!("response: {:#?}", transfer_response);
    // sleep(Duration::from_secs(30)).await;
    // tracing::warn!("finish");
}
