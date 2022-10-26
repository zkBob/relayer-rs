use relayer_rs::types::wallet_screening_request::WalletScreeningRequest;
use serde_json::json;
use wiremock::{
    matchers::{method, path},
    Mock, ResponseTemplate,
};

use crate::helpers::spawn_app;

#[actix_rt::test]
pub async fn test_trm_route() {
    let app = spawn_app(false).await.unwrap();

    let service_path  = "/trm_mock".to_string();
    Mock::given(method("POST"))
        .and(path(&service_path))
        // .and(header("Authorization","Basic "))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(
            {
                "accountExternalId": "00aa9688-dec6-47fa-be77-b0b2760d57f9",
                "address": "0xdac17f958d2ee523a2206206994597c13d831ec7",
                "addressRiskIndicators": [
                  {
                    "category": "Decentralized Exchange",
                    "categoryId": "6",
                    "categoryRiskScoreLevel": 5,
                    "categoryRiskScoreLevelLabel": "Medium",
                    "incomingVolumeUsd": "1111.11",
                    "outgoingVolumeUsd": "2222.22",
                    "riskType": "COUNTERPARTY",
                    "totalVolumeUsd": "3333.33"
                  },
                  {
                    "category": "Decentralized File Sharing Service",
                    "categoryId": "4",
                    "categoryRiskScoreLevel": 1,
                    "categoryRiskScoreLevelLabel": "Low",
                    "incomingVolumeUsd": "4444.44",
                    "outgoingVolumeUsd": "5555.55",
                    "riskType": "INDIRECT",
                    "totalVolumeUsd": "9999.99"
                  },
                  {
                    "category": "Sanctions",
                    "categoryId": "69",
                    "categoryRiskScoreLevel": 10,
                    "categoryRiskScoreLevelLabel": "High",
                    "incomingVolumeUsd": "6666.66",
                    "outgoingVolumeUsd": "7777.77",
                    "riskType": "OWNERSHIP",
                    "totalVolumeUsd": "14444.43"
                  }
                ],
                "addressSubmitted": "0xdAC17F958D2ee523a2206206994597C13D831ec7",
                "chain": "ethereum",
                "entities": [
                  {
                    "category": "Cold Wallet",
                    "categoryId": "71",
                    "entity": "Tether Treasury",
                    "riskScoreLevel": 42,
                    "riskScoreLevelLabel": "Low",
                    "trmAppUrl": "https://app.trmlabs.com/entities/trm/ca389716-cf85-4e99-98eb-0e9b3fb1415c",
                    "trmUrn": "/entity/manual/ca389716-cf85-4e99-98eb-0e9b3fb1415c"
                  }
                ],
                "trmAppUrl": "https://app.trmlabs.com/address/0xdac17f958d2ee523a2206206994597c13d831ec7/eth"
              }
        )))
        .mount(&app.mock_server)
        .await;

    let screening_service_endpoint = format!("{}{}", &app.address,"/wallet_screening");

    tracing::warn!("screening_service_endpoint: {}", screening_service_endpoint);
    let middleware_response = reqwest::Client::new()
        .post(screening_service_endpoint)
        .json(&WalletScreeningRequest {
            account_external_id: Some("foo".to_string()),
            address: "bar".to_string(),
            chain: "baz".to_string(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status();

    assert!(middleware_response.is_ok());
}
