use actix_web::{
    web::{self, Data},
    HttpResponse,
};

use crate::{state::State, types::wallet_screening_request::WalletScreeningRequest};

use reqwest;

pub async fn get_wallet_screening_result<D: kvdb::KeyValueDB>(
    request: web::Data<WalletScreeningRequest>,
    state: Data<State<D>>,
) -> Result<HttpResponse, actix_web::Error> {
    let web3 = &state.web3;
    let api_key = &web3.credentials.trm_api_key;

    let response = reqwest::Client::new()
    .post(&web3.trm_endpoint)
    .body(serde_json::to_string(&request).unwrap())
    .basic_auth(api_key, Some(api_key))
    .send()
    .await
    .unwrap()
    .error_for_status();


    match response   {
        Ok(res) =>  {
            let response_body_json = res.json().await.unwrap();
            Ok(HttpResponse::Ok().finish())
        },
        Err(e) => {
            tracing::error!("trm returned error :\n\t{:#?}", e);
            Err(actix_web::error::ErrorInternalServerError(e))
        }
    }

    
}
