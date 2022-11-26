use serde::{Deserialize, Serialize};
use serde_aux::field_attributes::deserialize_number_from_string;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub application: ApplicationSettings,
    pub web3: Web3Settings,
    pub trm: TRMSettings,
    pub custody: CustodyServiceSettings,
}

#[derive(Serialize, Deserialize, Clone,Debug)]
pub struct ApplicationSettings {
    pub host: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub telemetry: TelemetrySettings,
    pub tx: Tx,
    pub tree: Tree,
    pub db_path: String
    
}
#[derive(Debug,Deserialize, Serialize, Clone)]
pub enum TelemetryKind {
    Stdout,
    Jaeger
}

impl From<String> for TelemetryKind {
    fn from(s: String) -> Self {
        match s.as_str() {
            "stdout" => Self::Stdout,
            "jaeger" => Self::Jaeger,
            _ => Self::Stdout
        }
    }
}
#[derive(Debug,Deserialize, Serialize, Clone)]
pub struct TelemetrySettings {
    pub kind: TelemetryKind,
    pub endpoint: Option<String>,
    pub log_level: LogLevel
}

#[derive(Debug,Deserialize,Clone, Serialize, Copy)]
pub enum LogLevel {
    TRACE,
    DEBUG,
    INFO,
    WARN,
    ERROR
}


impl From<String> for LogLevel {
    
    fn from(s: String) -> Self {
        match  s.as_str() {
            "TRACE" => Self::TRACE,
            "DEBUG" => Self::DEBUG,
            "INFO" => Self::INFO,
            "WARN" => Self::WARN,
            "ERROR" => Self::ERROR,
            _ => Self::INFO
        }
    }
}

impl Into<String> for LogLevel {
    fn into(self) -> String {
        match self {
            LogLevel::TRACE => "TRACE",
            LogLevel::DEBUG => "DEBUG",
            LogLevel::INFO => "INFO",
            LogLevel::WARN => "WARN",
            LogLevel::ERROR => "ERROR",
        }.to_string()
    }
}



#[derive(Deserialize, Debug, Clone)]
pub struct Credentials {
    pub secret_key: String,
}

#[derive(Serialize,Deserialize,Clone,Debug)]
pub struct Web3Settings {
    pub provider_endpoint: String,
    pub abi_path: String,
    pub pool_address: String,
    pub relayer_fee: u64,
    pub gas_limit: u64,
    #[serde(skip_serializing)]
    pub credentials: Credentials,
    pub scheduler_interval_sec: u64,
    pub start_block: Option<u64>,
    pub batch_size: u64
}

#[derive(Serialize,Deserialize,Clone,Debug)]
pub struct TRMSettings {
    pub host: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub path: String,
    pub api_key: String
}

use libzeropool::fawkes_crypto::backend::bellman_groth16::{engines::Bn256, verifier, Parameters};

use crate::custody::config::CustodyServiceSettings;

impl ApplicationSettings {
    pub fn get_tx_vk(&self) -> Result<verifier::VK<Bn256>, std::io::Error> {

        let base_path = std::env::current_dir().expect("failed to determine current dir");
        
        let vk_path = base_path.join(&self.tx.vk);
        tracing::info!("vk_path= {}", vk_path.as_os_str().to_string_lossy());
        let vk_file = std::fs::File::open(base_path.join(&self.tx.vk))?;
        let vk: verifier::VK<Bn256> = serde_json::from_reader(vk_file)?;
        Ok(vk)
    }

    pub fn get_tree_params(&self) -> Parameters<Bn256> {
        let data = std::fs::read(self.tree.params.clone()).unwrap();
        Parameters::<Bn256>::read(&mut data.as_slice(), true, true).unwrap()
    }
    pub fn get_tx_params(&self) -> Parameters<Bn256> {
        let data = std::fs::read(self.tx.params.clone()).unwrap();
        Parameters::<Bn256>::read(&mut data.as_slice(), true, true).unwrap()
    }
}

#[derive(Serialize, Deserialize, Clone,Debug)]
pub struct Tx {
    pub params: String,
    pub client_mock_key: Option<String>,
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
    Staging,
    Mock,
    Anvil,
    Docker
}

impl Environment {
    fn as_str(&self) -> &'static str {
        match self {
            Environment::Local => "local.yaml",
            Environment::Staging => "staging.yaml",
            Environment::Production => "production.yaml",
            Environment::Mock => "mock.yaml",
            Environment::Anvil => "anvil.yaml",
            Environment::Docker => "docker.yaml",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            "staging" => Ok(Self::Staging),
            "mock" => Ok(Self::Mock),
            "anvil" => Ok(Self::Anvil),
            "docker" => Ok(Self::Docker),
            _other => Err(format!("failed to parse {}", s)),
        }
    }
}
