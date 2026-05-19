use std::sync::Arc;

use anyhow::Result;
use log4rs::{append::console::ConsoleAppender, config::*, encode::json::JsonEncoder, init_config};
use tokio::{join, sync::mpsc};

mod aggregator;
mod k8s;
mod model;
mod probe;
mod slack;

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls aws-lc-rs crypto provider");

    init_logger();

    let client = kube::Client::try_default().await?;
    let namespace = env("NAIS_NAMESPACE");
    let (tx, mut rx) = mpsc::channel::<(model::Log, String, String)>(100);

    let slack = Arc::new(slack::Slack::default());
    let window_seconds: i64 = env_or("AGGREGATE_WINDOW_SECONDS", 600);
    let edit_throttle_ms: u64 = env_or("AGGREGATE_EDIT_THROTTLE_MS", 5000);
    let aggregator = aggregator::Aggregator::new(slack.clone(), window_seconds, edit_throttle_ms);
    let _flush_handle = aggregator.clone().spawn_flush();

    let log_consumer = {
        let aggregator = aggregator.clone();
        tokio::spawn(async move {
            while let Some((log, container_name, pod_name)) = rx.recv().await {
                log::info!("found {:?}", &log);
                aggregator.ingest(log, container_name, pod_name).await;
            }
        })
    };

    let pod_controller = k8s::watch_pods(client, &namespace, tx);
    let health_probe = probe::health_check_server();

    let (consumer_res, controller_res, health_res) =
        join!(log_consumer, pod_controller, health_probe);

    controller_res?;
    health_res?;
    consumer_res?;

    Ok(())
}

pub fn env(env: &str) -> String {
    std::env::var(env).unwrap_or_else(|_| panic!("env var {} missing", env))
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn init_logger() {
    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(JsonEncoder::new()))
        .build();

    let config = log4rs::Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .logger(Logger::builder().build("app::logs", log::LevelFilter::Info))
        .build(Root::builder().appender("stdout").build(log::LevelFilter::Info))
        .expect("Failed to build log config");

    init_config(config).expect("Failed to init logger");
}
