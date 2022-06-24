use std::str::FromStr;
use ethabi::ethereum_types::{Address, H256};
use ethereum_jsonrpc::{types::BlockNumber, BlockFilter, LogFilter};
use jsonrpsee::http_client::HttpClient;
use kvdb_memorydb::InMemory;
use libzeropool::{native::params::PoolBN256, POOL_PARAMS};
use libzeropool_rs::merkle::MerkleTree;
use relayer_rs::{configuration::get_config, startup::get_events};
use serde_json::json;
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

    let events = get_events(
        db,
        &config.web3,
        None,
        None,
        Some(
            web3::types::H256::from_str(
                "0x3bda5fcdebd7ad010b2660e123d5cdf7d2a71246db4cc8b933f7c96792d9a94b",
            )
            .unwrap(),
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        events[0].transaction_hash,
        Some(
            web3::types::H256::from_str(
                "0x711ff43278149c796cfe596740caa85152dc67ac80f0b1590e63859e0ec5f9ac",
            )
            .unwrap(),
        ),
    );
    // println!("events: {:#?}", events);
}

#[tokio::test]
async fn test_logs() {
    env_logger::init();
    use ethereum_jsonrpc::EthApiClient;
    use jsonrpsee::http_client::HttpClientBuilder;
    let target = "https://mainnet.infura.io:443/v3/9a94d181b23846209f01161dcd0f9ad6";
    let client: HttpClient = HttpClientBuilder::default().build(target).unwrap();
    let events = client
        .get_logs(LogFilter {
            block_filter: BlockFilter::Exact {
                block_hash: H256::from_str(
                    "0x3bda5fcdebd7ad010b2660e123d5cdf7d2a71246db4cc8b933f7c96792d9a94b",
                )
                .unwrap(),
            },
            address: Some(vec![Address::from_str("0x21873d8fe216e5c0eea4ae948d9768b64f38e89b").unwrap()]),
            topics: Some(vec![H256::from_str(
                "0x7d39f8a6bc8929456fba511441be7361aa014ac6f8e21b99990ce9e1c7373536",
            )
            .unwrap()]),
        })
        .await
        .unwrap();

    println!("events = {:#?}", events);
}

#[tokio::test]
async fn test_get_block_by_num() {
    env_logger::init();
    use ethereum_jsonrpc::EthApiClient;
    use jsonrpsee::http_client::HttpClientBuilder;
    let target = "https://mainnet.infura.io:443/v3/9a94d181b23846209f01161dcd0f9ad6";
    let client: HttpClient = HttpClientBuilder::default().build(target).unwrap();
    if let Some(response) = client
        .get_block_by_number(
            BlockNumber::Number(ethabi::ethereum_types::U64([11 as u64])),
            false,
        )
        .await
        .unwrap()
    {}
}

#[test]
fn test_infura_response() {
    serde_json::from_value::<ethereum_jsonrpc::types::Block>(json!({
          "difficulty": "0x3ff7fb00a",
          "extraData": "0x476574682f76312e302e302d30636463373634372f6c696e75782f676f312e34",
          "gasLimit": "0x1388",
          "gasUsed": "0x0",
          "hash": "0x3f5e756c3efcb93099361b7ddd0dabfeaa592439437c1c836e443ccb81e93242",
          "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
          "miner": "0x19dafe19f11e960e4ccfc6a5aa8890ebd748ca1e",
          "mixHash": "0x5b1f9cc92e652b9448840c6ae40a63d2cdc2f360eed6dfb917a84f3e85a80feb",
          "nonce": "0x23447ad120ba5531",
          "number": "0xb",
          "parentHash": "0x4ff4a38b278ab49f7739d3a4ed4e12714386a9fdf72192f2e8f7da7822f10b4d",
          "receiptsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
          "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
          "size": "0x220",
          "stateRoot": "0x03f930c087b70f3385db68fe6bf128719e2d9a4b0a133e53b32db2fa25d345fd",
          "timestamp": "0x55ba42b8",
          "totalDifficulty": "0x2ff3004033",
          "transactions": [],
          "transactionsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
          "uncles": []
        })).unwrap();
}

#[test]
fn test_parse_event() {
    serde_json::from_value::<ethereum_jsonrpc::types::TransactionLog>(json!(
        {
            "address": "0x21873d8fe216e5c0eea4ae948d9768b64f38e89b",
            "blockHash": "0x3bda5fcdebd7ad010b2660e123d5cdf7d2a71246db4cc8b933f7c96792d9a94b",
            "blockNumber": "0x1e5f6e9",
            "data": "0x000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000ca010000005ccad6c5dd0d3b9d8ad460dad7ed24e67466bea4e4970485abafcf258fcbed0d068fda63a0b84455d816b47dbe0cf39fa505c95dbc683a9c69a9541177d6271b44596e1031865f73ab5fa746826839ff2d8ccc67319d13c67cd8f6be25241981a406d2be18c40bae16bd2d7a1098b581daa749484beb645af3f7c86a3149164bad5ca3552eb4caef841ed062663a725177eb00e7a61744d211dffbf89057808b8cba924eb4b76546590e22e96f3c4c73ad8faed61947c8203258fe5dc6eb4eaf003bef81e85200000000000000000000000000000000000000000000",
            "logIndex": "0x0",
            "removed": false,
            "topics": [
                "0x7d39f8a6bc8929456fba511441be7361aa014ac6f8e21b99990ce9e1c7373536",
                "0x0000000000000000000000000000000000000000000000000000000000000080",
                "0x7f06cfc68bb5e1be577b60b335f9c898acdae0eae27daa387a8dfa805d472762"
            ],
            "transactionHash": "0x711ff43278149c796cfe596740caa85152dc67ac80f0b1590e63859e0ec5f9ac",
            "transactionIndex": "0x0",
            "transactionLogIndex": "0x0",
            "type": "mined"
        }
    )).unwrap();
}
