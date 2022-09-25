use tracing::subscriber::set_global_default;
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn get_subscriber<'a>(
    name: String,
    env_filter: String,
    sink: impl MakeWriter<'a> + Send + Sync + 'static,
) -> impl Subscriber + Send + Sync {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));

    // let formatting_layer = BunyanFormattingLayer::new(name, sink);

    let subscriber = Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer);
        // .with(formatting_layer);

    subscriber
}

pub fn init_subscriber(subscriber: impl Subscriber + Send + Sync) {
    set_global_default(subscriber).expect("failed to get subscriber");
}
