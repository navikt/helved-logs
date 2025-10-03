use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Log {
    level: String,
    #[serde(rename = "@timestamp")]
    timestamp: Option<String>,
    logger_name: Option<String>,
    message: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    #[serde(rename = "HOSTNAME")]
    hostname: Option<String>,
}

impl Log {
    pub fn is_error(&self) -> bool {
        self.level == "ERROR"
    }

    pub fn to_slack_alert(&self, container: String, pod: String) -> serde_json::Value {
        let cluster = crate::env("NAIS_CLUSTER_NAME");
        let datasource = match cluster.as_str() {
            "dev-gcp" => "22P95CC91DC09CABFC8", 
            "prod-gcp" => "22P8A28344D07741F8D", 
            _ => {
                println!("could not resolve cluster name, default to dev");
                "22P95CC91DC09CABFC8"
            }
        };
        let trace_id = self.trace_id.clone().unwrap_or("".into());
        let grafana_trace_url = format!("https://grafana.nav.cloud.nais.io/explore?schemaVersion=1&panes=%7B%22trace%22%3A%7B%22datasource%22%3A%{datasource}%22%2C%22queries%22%3A%5B%7B%22queryType%22%3A%22traceql%22%2C%22query%22%3A%22{trace_id}%22%7D%5D%7D%7D");
        let peisen_url = match cluster.as_str() {
            "dev-gcp" => "https://peisen.intern.dev.nav.no/kafka",
            "prod-gcp" => "https://peisen.intern.nav.no/kafka",
            _ => {
                println!("could not resolve cluster name, default to dev");
                "https://peisen.intern.dev.nav.no/kafka"
            },
        };

        let timestamp = "2025-10-02T19:12:45.229595Z";
        let team_logs_url = match cluster.as_str() {
            "dev-gcp"  => format!("https://console.cloud.google.com/logs/query;query=resource.type%3D%22k8s_container%22%0Aresource.labels.cluster_name%3D%22prod%22%0Aresource.labels.container_name%3D%22{container}%22%0Aresource.labels.namespace_name%3D%22helved%22%0Aseverity%3E%3DERROR;cursorTimestamp={timestamp};duration=PT1H?project=helved-dev-9e3f"),
            "prod-gcp" => format!("https://console.cloud.google.com/logs/query;query=resource.type%3D%22k8s_container%22%0Aresource.labels.cluster_name%3D%22prod%22%0Aresource.labels.container_name%3D%22{container}%22%0Aresource.labels.namespace_name%3D%22helved%22%0Aseverity%3E%3DERROR;cursorTimestamp={timestamp};duration=PT1H?project=helved-prod-119e"),
            _ => {
                println!("could not resolve cluster name, default to dev");
                "https://console.cloud.google.com/logs/query;query=resource.type%3D%22k8s_container%22%0Aresource.labels.cluster_name%3D%22prod%22%0Aresource.labels.container_name%3D%22{container}%22%0Aresource.labels.namespace_name%3D%22helved%22%0Aseverity%3E%3DERROR;cursorTimestamp={timestamp};duration=PT1H?project=helved-dev-9e3f".into()
            },
        };

        json!({
            "blocks": [
                {
                    "type": "header",
                    "text": {
                        "type": "plain_text",
                        "text": format!(":wood: {}", container),
                        "emoji": true
                    }
                },
                {
                    "type": "rich_text",
                    "elements": [
                        {
                            "type": "rich_text_section",
                            "elements": [
                                {
                                    "type": "text",
                                    "text": pod,
                                    "style": {
                                        "italic": true
                                    }
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": "section",
                    "text": {
                        "type": "plain_text",
                        "text": cluster,
                        "emoji": true
                    }
                },
                {
                    "type": "divider"
                },
                {
                    "type": "section",
                    "text": {
                        "type": "plain_text",
                        "text": ":firecracker: :firecracker: :firecracker: :firecracker: :firecracker: :firecracker: :firecracker:",
                        "emoji": true
                    }
                },
                {
                    "type": "section",
                    "text": {
                        "type": "plain_text",
                        "text": self.logger_name,
                        "emoji": true
                    }
                },
                {
                    "type": "rich_text",
                    "elements": [
                        {
                            "type": "rich_text_preformatted",
                            "elements": [
                                {
                                    "type": "text",
                                    "text": self.message
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": "section",
                    "text": {
                        "type": "plain_text",
                        "text": ":firecracker: :firecracker: :firecracker: :firecracker: :firecracker: :firecracker: :firecracker:",
                        "emoji": true
                    }
                },
                {
                    "type": "divider"
                },
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!("trace_id: {}", trace_id)
                    },
                    "accessory": {
                        "type": "button",
                        "text": {
                            "type": "plain_text",
                            "text": "grafana"
                        },
                        "url": grafana_trace_url,
                        "action_id": "button-action"
                    }
                },
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!("Filterer på {} i team logs", container)
                    },
                    "accessory": {
                        "type": "button",
                        "text": {
                            "type": "plain_text",
                            "text": "secure log"
                        },
                        "url": team_logs_url,
                        "action_id": "button-action"
                    }
                },
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": "Filtrer på trace_id"
                    },
                    "accessory": {
                        "type": "button",
                        "text": {
                            "type": "plain_text",
                            "text": "peisen"
                        },
                        "url": peisen_url,
                        "action_id": "button-action"
                    }
                }
            ]
        })
    }
}
