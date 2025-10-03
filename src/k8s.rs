use std::{collections::HashMap};

use anyhow::Result;
use futures::{AsyncBufReadExt, StreamExt, TryStreamExt};
use k8s_openapi::{api::core::v1::Pod};
use kube::{api::{Api, LogParams}, runtime::{watcher}, Client, ResourceExt };
use tokio::{sync::mpsc::{Sender}, time::Duration, task::{AbortHandle}};

use crate::model::Log;

pub async fn watch_pods(
    client: Client,
    namespace: &str,
    tx: Sender<(Log, String, String)>,
) -> Result<()> {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let wc = watcher::Config::default(); //.streaming_lists(); // krever feature WatchList i K8s
    let mut events = watcher(api.clone(), wc).boxed();
    let mut log_tasks: HashMap<String, AbortHandle> = HashMap::new();
    let self_name = std::env::var("NAIS_APP_NAME").unwrap_or("local".into());

    while let Some(event) = events.try_next().await? {
        match event {
            watcher::Event::InitApply(pod) | watcher::Event::Apply(pod) => {
                let pod_name = pod.name_any();

                if pod_phase(&pod) == "Running" && !log_tasks.contains_key(&pod_name) {
                    let containers = pod.spec
                        .map(|spec| spec.containers)
                        .map(|containers| containers.into_iter().map(|c|c.name).collect::<Vec<String>>())
                        .unwrap_or_default();

                    for container_name in containers {
                        if container_name == self_name { continue;}
                        let pods_clone = api.clone();
                        let tx_clone = tx.clone();
                        let pod_name_clone = pod_name.clone();
                        let handle = tokio::spawn(async move {
                            match watch_logs(container_name, pod_name_clone, pods_clone, tx_clone).await {
                                    Ok(_) => (),
                                    Err(e) => eprintln!("Task error {}", e),
                                }
                        });
                        log_tasks.insert(pod_name.clone(), handle.abort_handle());
                        println!("start log for {}", pod_name);
                    }
                }
            }
            watcher::Event::Delete(pod) => {
                let pod_name = pod.name_any();
                if let Some(handle) = log_tasks.remove(&pod_name) {
                    handle.abort();
                    println!("stop logs for {} because pod was deleted", pod.name_any());
                }
            },
            watcher::Event::Init => {}
            watcher::Event::InitDone => {}
        }
    }

    Ok(())
}

fn pod_phase(pod: &Pod) -> &str {
    pod.status.as_ref()
        .and_then(|s| s.phase.as_ref())
        .map(|s|s.as_str())
        .unwrap_or("unknown")
}

async fn watch_logs(
    container_name: String,
    pod_name: String,
    pods: Api<Pod>,
    tx: Sender<(Log, String, String)>,
) -> Result<()> {
    loop {
        let params = LogParams {
            container: Some(container_name.clone()),
            tail_lines: Some(0), 
            timestamps: false, 
            follow: true,
            ..LogParams::default()
        };

        match pods.log_stream(&pod_name, &params).await {
            Ok(logs) => {
                let mut lines = logs.lines();

                while let Some(line_result) = lines.next().await {
                    match line_result {
                        Ok(line) => {
                            if let Some(json_start_idx) = line.find('{') {
                                let json_part = &line[json_start_idx..];
                                match serde_json::from_str::<Log>(json_part) {
                                    Ok(log) => {
                                        // if !&log.is_error() {
                                        //     println!("found non error log {:?}", &log);
                                        // }
                                        if log.is_error() && tx.send((log, container_name.clone(), pod_name.clone())).await.is_err() { 
                                            return Ok(());
                                        }
                                    }
                                    Err(e) => eprintln!("json {}: {}", container_name, e),
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("line_result {}: {}, reconnects..", container_name, e);
                            break;
                        }
                    }
                }
                return Ok(());
            }
            Err(e) => {
                eprintln!("log stream {}: {}", container_name, e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

