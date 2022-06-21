use crate::helpers::spawn_app;
use kvdb_memorydb::InMemory;
use libzeropool::{native::params::PoolBN256, POOL_PARAMS};
use libzeropool_rs::merkle::MerkleTree;
use relayer_rs::{configuration::get_config, startup::get_events};
#[test]
fn keccak_test() {
    use hex_literal::hex;
    use web3::signing::keccak256;

    let a = keccak256("Transfer(address,address,uint256)".as_bytes());

    assert_eq!(
        a,
        hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")
    )
}

#[tokio::test]
async fn test_get_events() {

    env_logger::init();
    
    let db: MerkleTree<InMemory, PoolBN256> = MerkleTree::new_test(POOL_PARAMS.clone());

    let config = get_config().unwrap();

    let events = get_events(db, &config.web3).await.unwrap();

    println!("events: {:#?}", events);
}


#[tokio::test]
async fn test_json_rpc() {
    
}