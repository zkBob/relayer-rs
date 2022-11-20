use opentelemetry::global;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Registry};

use crate::configuration::Settings;

pub fn init_stdout(name: String, env_filter: String) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));

    let formatting_layer = BunyanFormattingLayer::new(name, || std::io::stdout());
    Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer)
        .init();
}

pub fn init_sink(name: String, env_filter: String) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));

    let formatting_layer = BunyanFormattingLayer::new(name, || std::io::sink());
    Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer)
        .init();
}

use opentelemetry::runtime::Tokio;

pub fn init_jaeger(name: String, log_level: String, endpoint: &Option<String>) {
    
    global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

    let mut agent_pipeline = opentelemetry_jaeger::new_agent_pipeline().with_service_name(name);

    if let Some(agent_endpoint) = endpoint {
        agent_pipeline = agent_pipeline.with_endpoint(agent_endpoint);
    }

    let tracer = agent_pipeline.install_batch(Tokio).unwrap();

    Registry::default()
        .with(tracing_subscriber::EnvFilter::new(log_level))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .init();
}

pub fn setup_telemetry(config: &Settings) {
    let name = "zkbob-relayer".to_string();
    let telemetry_settings = &config.application.telemetry;
    let log_level = telemetry_settings.log_level.into();

    match config.application.telemetry.kind {
        crate::configuration::TelemetryKind::Stdout => init_stdout(name, log_level),
        crate::configuration::TelemetryKind::Jaeger => {
            init_jaeger(name, log_level, &telemetry_settings.endpoint)
        }
    }
}
