use serde::{Deserialize, Serialize};
use serde_aux::field_attributes::deserialize_number_from_string;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub application: ApplicationSettings,
    pub web3: Web3Settings
}

#[derive(Serialize, Deserialize, Clone,Debug)]
pub struct ApplicationSettings {
    pub host: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub tx: Tx,
    pub tree: Tree,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Credentials {
    _secret_key: String
}

#[derive(Serialize,Deserialize,Clone,Debug)]
pub struct Web3Settings {
    pub provider_endpoint: String,
    pub abi_path: String,
    pub pool_address: String,
    #[serde(skip_serializing)]
    pub credentials: Credentials
}

use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, verifier};

impl ApplicationSettings {
    pub fn get_tx_vk(&self) -> Result<verifier::VK<Bn256>, std::io::Error> {

        let base_path = std::env::current_dir().expect("failed to determine current dir");
        
        let vk_path = base_path.join(&self.tx.vk);
        tracing::info!("vk_path= {}", vk_path.as_os_str().to_string_lossy());
        let vk_file = std::fs::File::open(base_path.join(&self.tx.vk))?;
        let vk: verifier::VK<Bn256> = serde_json::from_reader(vk_file)?;
        Ok(vk)
    }
}

#[derive(Serialize, Deserialize, Clone,Debug)]
pub struct Tx {
    pub vk: String,
}

#[derive(Serialize, Deserialize, Clone,Debug)]
pub struct Tree {
    pub params: String,
}

pub fn get_config() -> Result<Settings, config::ConfigError> {
    let mut settings = config::Config::default();
    let base_path = std::env::current_dir().expect("failed to determine current dir");
    let configuration_directory = base_path.join("configuration");

    settings.merge(config::File::from(configuration_directory.join("base.yaml")).required(true))?;

    let environment: Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "local".into())
        .try_into()
        .expect("failed to parse environment");

    settings
        .merge(
            config::File::from(configuration_directory.join(environment.as_str())).required(true),
        )
        .expect("failed to apply env settings");

    settings.merge(config::Environment::with_prefix("app").separator("__"))?;
    settings.try_into()
}

pub enum Environment {
    Local,
    Production,
}

impl Environment {
    fn as_str(&self) -> &'static str {
        match self {
            Environment::Local => "local.yaml",
            Environment::Production => "production.yaml",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            _other => Err(format!("failed to parse {}", s)),
        }
    }
}
