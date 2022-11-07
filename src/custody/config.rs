use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustodyServiceSettings {
    pub accounts_path: String
}