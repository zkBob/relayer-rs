use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct WalletScreeningRequest {
    #[serde(alias = "accountExternalId")]
    pub account_external_id: Option<String>,

    pub address: String,

    pub chain: String,
}
#[derive(Serialize,Deserialize, Debug)]
pub struct TrmRequest(pub Vec<WalletScreeningRequest>);
#[derive(Serialize,Deserialize, Debug)]
pub struct TrmResponse (pub Vec<WalletScreeningResponse>);
#[derive(Serialize,Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WalletScreeningResponse {
    pub account_external_id: Option<String>,
    pub address: String,
    pub address_risk_indicators: Vec<AddressRiskIndicators>,
    pub address_submitted: String,
    pub chain: String,
    pub entities: Vec<Entity>,
    pub trm_app_url: String,
}

#[derive(Serialize,Deserialize,Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub category: String,
    pub category_id: String,
    pub entity: String,
    pub risk_score_level: u8,
    pub risk_score_level_label: String,
    pub trm_urn: String,
    pub trm_app_url: String,
}
#[derive(Serialize,Deserialize,Debug)]
pub enum RiskType {
    COUNTERPARTY,
    INDIRECT,
    OWNERSHIP,
}
#[derive(Serialize,Deserialize,Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddressRiskIndicators {
    pub category: String,
    pub category_id: String,
    pub category_risk_score_level: u8,
    pub category_risk_score_level_label: String,
    pub incoming_volume_usd: String,
    pub outgoing_volume_usd: String,
    pub risk_type: RiskType,
    pub total_volume_usd: String,
}
