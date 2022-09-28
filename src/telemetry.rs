use opentelemetry::sdk::trace::Tracer;
use tracing::subscriber::set_global_default;
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
// use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};
use opentelemetry::{global, sdk::propagation::TraceContextPropagator};
use opentelemetry_jaeger::new_agent_pipeline;
use tracing_subscriber::prelude::*;

// pub fn get_subscriber<'a,W:std::io::Write>(
//     name: String,
//     env_filter: String,
//     sink:   &dyn (Fn() ->  W ) //impl MakeWriter<'a> + Send + Sync + 'static,
// ) -> impl Subscriber + Send + Sync {
//     let env_filter =
//         EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));

//     let subscriber = Registry::default()
//         .with(env_filter)
//         .with(JsonStorageLayer);
        
//     subscriber
// }

pub fn stdout_subscriber(name: String,env_filter: String)  -> impl Subscriber + Send + Sync {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));
    
        let formatting_layer = BunyanFormattingLayer::new(name, ||std::io::stdout());
    let subscriber = Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer);
        

    subscriber
}

pub fn sinc_subscriber(name: String,env_filter: String)  -> impl Subscriber + Send + Sync {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));
    
        let formatting_layer = BunyanFormattingLayer::new(name, ||std::io::sink());
    let subscriber = Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .with(JsonStorageLayer);
        

    subscriber
}

pub fn jaeger_subscriber(tracer: Tracer) -> impl Subscriber + Send + Sync{

    global::set_text_map_propagator(TraceContextPropagator::new());

        let subscriber = Registry::default()
            .with(tracing_subscriber::EnvFilter::new("INFO"))
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_opentelemetry::layer().with_tracer(tracer));

       subscriber

}

pub fn init_subscriber2(subscriber: impl Subscriber + Send + Sync) {
    tracing::info_span!("WTF!!!");
    subscriber.init();
}

pub fn init_subscriber(subscriber: impl Subscriber + Send + Sync) {
    set_global_default(subscriber).expect("failed to get subscriber");
}
