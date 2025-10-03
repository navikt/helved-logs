use anyhow::Result;
use tokio::{join, sync::mpsc::{self}};

mod k8s;
mod model;
mod slack;
mod probe;

#[tokio::main]
async fn main() -> Result<()> {
    let client = kube::Client::try_default().await?;
    let namespace = env("NAIS_NAMESPACE");
    let (tx, mut rx) = mpsc::channel::<(model::Log, String, String)>(100);
    let slack = slack::Slack::default();

    let log_consumer = tokio::spawn(async move {
        while let Some((log, container_name, pod_name))  = rx.recv().await {
            println!("found {:?}", &log);
            match slack.send(log, container_name, pod_name).await {
                Ok(res) if res.status().as_u16() == 200  => println!("sent"),
                _ => println!("failed to send"),
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

