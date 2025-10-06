use anyhow::Result;
use tokio::{join, sync::mpsc::{self}};
use log4rs::{config::*, encode::json::JsonEncoder, init_config, append::console::ConsoleAppender};

mod k8s;
mod model;
mod slack;
mod probe;

#[tokio::main]
async fn main() -> Result<()> {
    init_logger();

    let client = kube::Client::try_default().await?;
    let namespace = env("NAIS_NAMESPACE");
    let (tx, mut rx) = mpsc::channel::<(model::Log, String, String)>(100);
    let slack = slack::Slack::default();

    let log_consumer = tokio::spawn(async move {
        while let Some((log, container_name, pod_name))  = rx.recv().await {
            log::info!("found {:?}", &log);
            match slack.send(log, container_name, pod_name).await {
                Ok(_) => log::info!("sent"),
                Err(e) => log::info!("failed {}", e),
            }
        }
    });

    let pod_controller = k8s::watch_pods(client, &namespace, tx);

    let health_probe = probe::health_check_server();

    let (consumer_res, controller_res, health_res) = join!(
        log_consumer, 
        pod_controller,
        health_probe 
    );

    controller_res?;
    health_res?;
    consumer_res?;

    Ok(())
}

pub fn env(env: &str) -> String {
    std::env::var(env).unwrap_or_else(|_| panic!("env var {} missing", env))
}

fn init_logger() {
    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(JsonEncoder::new()))
        .build();

    let config = log4rs::Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .logger(Logger::builder().build("app::logs", log::LevelFilter::Info))
        .build(Root::builder().appender("stdout").build(log::LevelFilter::Debug))
        .expect("Failed to build log config");

    init_config(config).expect("Failed to init logger");
}

