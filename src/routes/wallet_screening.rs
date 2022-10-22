use actix_web::{
    web::{self, Data},
    HttpResponse,
};

use crate::{
    state::State,
    types::wallet_screening_request::{WalletScreeningRequest, WalletScreeningResponse},
};

use reqwest;

use super::ServiceError;

pub async fn make_trm_request(
    request: WalletScreeningRequest,
    endpoint: &str,
    api_key: &str,
) -> Result<WalletScreeningResponse, ServiceError> {

    let response = reqwest::Client::new()
        .post(endpoint)
        .body(serde_json::to_string(&request).unwrap())
        .basic_auth(&api_key, Some(&api_key))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .map_err(|e| {
            tracing::error!("error caught {:#?}", e);
            ServiceError::InternalError
        })?;

    response
        .json::<WalletScreeningResponse>()
        .await
        .map_err(|_| ServiceError::InternalError)
}
pub async fn get_wallet_screening_result<D: kvdb::KeyValueDB>(
    request: web::Json<WalletScreeningRequest>,
    state: Data<State<D>>,
) -> Result<HttpResponse, actix_web::Error> {
    tracing::info!("received request {:#?}", request);
    let web3 = &state.web3;
    let api_key = &web3.credentials.trm_api_key;
    let request:WalletScreeningRequest = request.0.into();
    make_trm_request(request, &web3.trm_endpoint, api_key.as_str())
        .await
        .map(|v| HttpResponse::Ok().json(v))
        .map_err(|e| {

            tracing::error!("tmr returned error {:#?}", e );
            e.into()
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::types::wallet_screening_request::TrmResponse;

    #[test]
    fn test_parse() {
        let mock_response_as_json = json!([
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
        ]);
        let wallet_screening_response =
            serde_json::from_value::<TrmResponse>(mock_response_as_json);
        println!("wallet_screening_response:{:#?}", wallet_screening_response);
        assert!(wallet_screening_response.is_ok());
    }
}
