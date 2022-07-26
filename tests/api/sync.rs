use actix_web::web::Data;
use ethabi::ethereum_types::{Address, H256};
use ethereum_jsonrpc::{BlockFilter, LogFilter};
use jsonrpsee::http_client::HttpClient;
use kvdb_memorydb::InMemory;
use libzeropool::{native::params::PoolBN256, POOL_PARAMS};
use libzeropool_rs::merkle::MerkleTree;
use relayer_rs::{
    configuration::get_config,
    contracts::Pool,
    startup::{get_events, DB},
};
use serde_json::json;
use std::{str::FromStr, sync::Mutex};
use web3::types::BlockNumber;

use crate::helpers::spawn_app;
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
async fn test_sync() {
    let app = spawn_app().await.unwrap();
}

#[tokio::test]
async fn test_get_events() {
    env_logger::init();

    let config = get_config().unwrap();

    let events = get_events(
        Some(BlockNumber::Earliest),
        Some(BlockNumber::Latest),
        None,
        &Pool::new(Data::new(config.web3)).unwrap(),
    )
    .await
    .unwrap();

    tracing::info!("events: {:#?}", events);
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
            address: Some(vec![Address::from_str(
                "0x21873d8fe216e5c0eea4ae948d9768b64f38e89b",
            )
            .unwrap()]),
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
            ethereum_jsonrpc::types::BlockNumber::Number(ethabi::ethereum_types::U64([11 as u64])),
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

#[test]
fn test_parse_memo() {
    let test_memo = [
        175, 152, 144, 131, 4, 45, 146, 65, 207, 30, 174, 247, 55, 124, 233, 161, 216, 83, 90, 75,
        188, 12, 101, 164, 33, 57, 11, 57, 183, 192, 36, 95, 27, 191, 132, 69, 0, 98, 36, 70, 222,
        3, 22, 226, 61, 18, 68, 131, 171, 130, 204, 54, 244, 160, 237, 46, 122, 139, 165, 75, 207,
        49, 211, 33, 95, 98, 47, 153, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 23, 72, 118, 232, 0, 24, 49, 62, 74, 91, 146, 188, 200, 144, 58, 237, 158, 34, 81,
        64, 163, 144, 148, 65, 236, 117, 189, 109, 248, 28, 36, 217, 111, 243, 208, 83, 34, 3, 66,
        102, 145, 249, 252, 197, 71, 227, 217, 19, 237, 120, 233, 117, 3, 88, 212, 42, 62, 103,
        153, 226, 151, 218, 122, 155, 189, 240, 94, 59, 227, 2, 75, 58, 238, 127, 119, 195, 191,
        158, 79, 228, 13, 13, 28, 102, 43, 17, 8, 104, 153, 83, 171, 177, 196, 123, 27, 121, 154,
        133, 149, 172, 103, 0, 4, 230, 32, 148, 222, 23, 181, 159, 56, 181, 78, 40, 96, 224, 196,
        15, 53, 175, 189, 108, 135, 10, 20, 63, 44, 151, 6, 161, 246, 107, 46, 42, 134, 176, 82,
        151, 27, 98, 240, 89, 123, 241, 227, 196, 71, 88, 11, 15, 71, 167, 229, 119, 57, 69, 204,
        230, 230, 218, 158, 135, 151, 56, 70, 39, 131, 223, 243, 129, 217, 112, 124, 112, 162, 212,
        21, 81, 252, 177, 189, 16, 31, 222, 55, 59, 109, 179, 57, 255, 184, 13, 206, 13, 24, 78,
        200, 45, 172, 95, 52, 248, 198, 219, 197, 55, 0, 63, 37, 122, 227, 223, 182, 70, 19, 231,
        145, 239, 177, 22, 182, 214, 19, 249, 12, 102, 190, 249, 49, 42, 244, 61, 225, 106, 89, 2,
        222, 150, 250, 125, 131, 197, 96, 85, 127, 207, 235, 65, 139, 29, 226, 81, 15, 225, 110,
        229, 51, 151, 135, 255, 123, 21, 102, 45, 117, 73, 181, 20, 195, 50, 12, 99, 159, 54, 81,
        52, 31, 248, 179, 103, 247, 122, 222, 139, 137, 67, 44, 224, 177, 27, 221, 40, 225, 8, 141,
        92, 75, 127, 5, 27, 202, 246, 188, 38, 248, 80, 151, 209, 221, 172, 103, 105, 218, 206,
        125, 69, 210, 17, 47, 55, 101, 112, 203, 72, 137, 39, 234, 184, 223, 88, 188, 7, 111, 61,
        152, 124, 196, 235, 124, 180, 18, 81, 194, 118, 67, 154, 247, 48, 83, 178, 79, 81, 60, 117,
        177, 186, 205, 14, 220, 156, 187, 117, 199, 41, 203, 98, 97, 47, 135, 173, 161, 187, 206,
        115, 181, 198, 90, 236, 92, 89, 14, 133, 74, 32, 248, 255, 93, 65, 139, 33, 133, 27, 30,
        70, 48, 40, 234, 241, 225, 85, 195, 246, 231, 207, 31, 40, 166, 149, 112, 16, 198, 164, 93,
        71, 224, 160, 171, 74, 33, 167, 40, 4, 246, 241, 34, 121, 18, 109, 180, 217, 189, 99, 173,
        112, 174, 144, 42, 56, 158, 119, 4, 80, 179, 94, 213, 96, 116, 10, 95, 126, 230, 95, 73,
        31, 78, 53, 119, 179, 249, 154, 131, 250, 144, 112, 30, 54, 93, 114, 106, 122, 105, 8, 197,
        58, 220, 61, 168, 166, 134, 114, 187, 108, 177, 30, 52, 42, 215, 157, 47, 72, 255, 254, 57,
        194, 28, 59, 58, 43, 76, 189, 72, 87, 250, 145, 90, 68, 55, 73, 6, 220, 52, 87, 84, 149,
        12, 20, 77, 22, 98, 248, 15, 42, 72, 219, 80, 138, 79, 43, 65, 225, 41, 226, 9, 241, 5, 89,
        62, 34, 43, 161, 16, 182, 232, 215, 156, 46, 1, 120, 175, 0, 3, 0, 238, 0, 0, 0, 0, 0, 152,
        150, 128, 0, 0, 0, 0, 98, 223, 209, 108, 255, 207, 143, 222, 231, 42, 193, 27, 92, 84, 36,
        40, 179, 94, 239, 87, 105, 196, 9, 240, 1, 0, 0, 0, 251, 218, 179, 31, 50, 58, 106, 5, 47,
        189, 186, 175, 144, 31, 59, 9, 149, 122, 94, 243, 208, 80, 4, 172, 118, 54, 46, 198, 104,
        159, 17, 16, 128, 249, 245, 169, 137, 230, 192, 240, 49, 158, 92, 228, 245, 139, 148, 70,
        172, 211, 48, 242, 20, 195, 136, 40, 165, 19, 139, 166, 195, 28, 196, 1, 46, 32, 84, 132,
        99, 45, 215, 162, 10, 46, 225, 4, 183, 113, 132, 127, 151, 141, 24, 95, 129, 210, 201, 156,
        250, 234, 36, 71, 220, 22, 242, 139, 221, 37, 68, 219, 203, 238, 15, 222, 17, 107, 134,
        179, 233, 20, 76, 188, 87, 176, 100, 128, 248, 246, 23, 173, 169, 45, 71, 58, 225, 99, 63,
        216, 220, 52, 243, 129, 98, 164, 95, 245, 226, 216, 83, 218, 29, 92, 148, 175, 101, 43, 40,
        187, 143, 96, 175, 159, 235, 52, 209, 115, 36, 237, 82, 60, 91, 181, 22, 188, 41, 18, 44,
        70, 218, 153, 235, 118, 170, 105, 176, 120, 24, 136, 83, 63, 146, 123, 251, 165, 202, 128,
        66, 219, 135, 73, 128, 51, 48, 149, 231, 71, 169, 201, 51, 232, 125, 245, 129, 145, 156, 3,
        93, 26, 124, 142, 103, 144, 229, 139, 232, 140, 41, 209, 211, 92, 28, 162, 232, 229, 158,
        183, 35, 46, 57, 122, 87, 140, 48, 127, 40, 145, 125, 229, 53, 215, 167, 62, 187, 186, 4,
        174, 197, 49, 84, 9, 146, 82, 140, 92, 92, 105, 229, 25, 16, 71, 215, 108, 27,
    ];

    /*TODO:

        cretate a parser for memo field

        type Field =
      'selector' |
      'nullifier' |
      'outCommit' |
      'txType' |
      'memoSize' |
      'memo'

    type FieldMapping = {
      [key in Field]: { start: number, size: number }
    }

    export class PoolCalldataParser {
      private fields: FieldMapping = {
        selector: { start: 0, size: 4 },
        nullifier: { start: 4, size: 32 },
        outCommit: { start: 36, size: 32 },
        txType: { start: 640, size: 2 },
        memoSize: { start: 642, size: 2 },
        memo: { start: 644, size: 0 },
      }
      constructor(private calldata: Buffer) { }

      getField(f: Field, defaultSize?: number) {
        let { start, size } = this.fields[f]
        size = defaultSize || size
        return '0x' + this.calldata.slice(start, start + size).toString('hex')
      }



        */
}
