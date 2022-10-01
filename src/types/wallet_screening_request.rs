use serde::{Serialize, Deserialize};

#[derive(Serialize,Deserialize)]
pub struct WalletScreeningRequest {
    #[serde(alias = "accountExternalId")]
    pub account_external_id  : Option<String>,

    pub address: String,

    pub chain: String
    
}