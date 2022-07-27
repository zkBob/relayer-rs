#[derive(Debug)]
pub enum TestError {
    NetworkError(reqwest::Error),
    GeneratorError(String),
    FileAccessError(std::io::Error),
    SerializationError(serde_json::Error),
    ConfigError(String),
    BadResponse(String),
    MpscError,
}

impl From<reqwest::Error> for TestError {
    fn from(e: reqwest::Error) -> Self {
        Self::NetworkError(e)
    }
}
impl From<std::io::Error> for TestError {
    fn from(e: std::io::Error) -> Self {
        Self::FileAccessError(e)
    }
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Adding a subscriber has failed")
    }
}

impl std::error::Error for TestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TestError::NetworkError(e) => Some(e),
            TestError::GeneratorError(_) => None,
            TestError::FileAccessError(e) => Some(e),
            TestError::SerializationError(e) => Some(e),
            TestError::ConfigError(_) => None,
            TestError::BadResponse(_) => None,
            TestError::MpscError => None,
        }
    }
}