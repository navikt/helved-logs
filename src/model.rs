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
        let trace_id = self.trace_id.clone().unwrap_or("".into());
        let grafana_trace_url = self.resolve_grafana_url(&cluster, &trace_id);
        let grafana_log_url = self.resolve_grafana_loki(&container, &cluster);
        let peisen_url = self.resolve_peisen_url(&cluster, &trace_id);
        let team_logs_url = self.resolve_team_logs_url(&cluster, &container);
        
        let cluster_label = match cluster.as_str() {
            "prod-gcp" => ":alert: PROD :alert:",
            _ => "DEV",
        };

        json!({
            "channel": "team-hel-ved-alerts",
            "blocks": [
                {
                    "type": "header",
                    "text": {
                        "type": "plain_text",
                        "text": format!(":code-on-fire: {}", container),
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
                        "text": cluster_label,
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
                        "text": format!("logger: {}", self.logger_name.clone().unwrap_or("log".into())),
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
                    "type": "divider"
                },
                {
                    "type": "section",
                    "text": {
                        "type": "plain_text",
                        "text": format!("trace_id: {}", trace_id),
                        "emoji": true
                    }
                },
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "text": {
                                "type": "plain_text",
                                "text": "trace :grafana:",
                                "emoji": true
                            },
                            "url": grafana_trace_url,
                            "action_id": "button-action"
                        }
                    ],
                },
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "text": {
                                "type": "plain_text",
                                "text": "open logs :grafana:",
                                "emoji": true
                            },
                            "url": grafana_log_url,
                            "action_id": "button-action"
                        }
                    ],
                },
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "text": {
                                "type": "plain_text",
                                "text": "secure logs :gcp:",
                                "emoji": true
                            },
                            "url": team_logs_url,
                            "action_id": "button-action"
                        }
                    ],
                },
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "text": {
                                "type": "plain_text",
                                "text": "peisen :wood:",
                                "emoji": true
                            },
                            "url": peisen_url,
                            "action_id": "button-action"
                        }
                    ],
                }
            ]
        })
    }

    fn resolve_team_logs_url(&self, cluster: &str, container: &str) -> String {
        use chrono::{DateTime, Utc, Duration};

        let host = "https://console.cloud.google.com";
        let cursor_timestamp = if let Some(ts) = &self.timestamp {
            match DateTime::parse_from_rfc3339(ts) {
                Ok(dt) => dt.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                Err(_) => (Utc::now() - Duration::minutes(2)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
            }
        } else {
            (Utc::now() - Duration::minutes(2)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
        };
        let namespace = crate::env("NAIS_NAMESPACE");
        let severity = "ERROR";
        let query = format!("resource.type%3D%22k8s_container%22%0Aresource.labels.container_name%3D%22{container}%22%0Aresource.labels.namespace_name%3D%22{namespace}%22%0Aseverity%3E%3D{severity}");
        let duration = "PT1H";
        let project = match cluster {
            "prod-gcp" => "helved-prod-119e",
            _ => "helved-dev-9e3f",
        };

        format!("{host}/logs/query;query={query};cursorTimestamp={cursor_timestamp};duration={duration}?project={project}")
    }

    fn resolve_grafana_url(&self, cluster: &str, trace_id: &str) -> String {
        let datasource = match cluster {
            "prod-gcp" => "22P8A28344D07741F8D", 
            _ => "22P95CC91DC09CABFC8", 
        };
        let host = "https://grafana.nav.cloud.nais.io";
        format!("{host}/explore?schemaVersion=1&panes=%7B%22trace%22%3A%7B%22datasource%22%3A%{datasource}%22%2C%22queries%22%3A%5B%7B%22queryType%22%3A%22traceql%22%2C%22query%22%3A%22{trace_id}%22%7D%5D%7D%7D")
    }

    fn resolve_peisen_url(&self, cluster: &str, trace_id: &str) -> String {
        use chrono::{Utc, Duration};

        let host = match cluster {
            "prod-gcp" => "https://peisen.intern.nav.no", 
            _ => "https://peisen.intern.dev.nav.no", 
        };

        let (fom, tom) = if let Some(timestamp) = &self.timestamp && let Ok((from, to)) = format_and_skew(timestamp.as_str(), "%Y-%m-%dT%H:%M:%S%.3fZ", 1) {
            (from, to)
        } else {
            let now = Utc::now();
            let fom = (now - Duration::minutes(1)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            let tom = (now + Duration::minutes(1)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            (fom, tom)
        };

        match trace_id {
            "" => format!("{host}/kafka?fom={fom}&tom={tom}"),
            _ => format!("{host}/kafka?fom={fom}&tom={tom}&trace_id={trace_id}")
        }
    }

    fn resolve_grafana_loki(&self, container: &str, cluster: &str) -> String {
        let host = "https://grafana.nav.cloud.nais.io";
        let var_ds = match cluster {
            "prod-gcp" => "PD969E40991D5C4A8", 
            _ => "P7BE696147D279490", 
        };
        let (from, to) = if let Some(timestamp) = &self.timestamp {
            match format_and_skew(timestamp.as_str(), "%Y-%m-%dT%H:%M:%S%.3fZ", 1) {
                Ok((from, to)) => (from, to),
                Err(_) => ("now-6h".to_string(), "now".to_string())
            }
        } else {
            ("now-6h".to_string(), "now".to_string())
        };
        format!("{host}/a/grafana-lokiexplore-app/explore/service/{container}/logs?from={from}&to={to}&var-ds={var_ds}&var-filters=service_name|%3D|{container}&patterns=[]&var-lineFormat=&var-fields=&var-levels=detected_level|%3D|Error&var-levels=detected_level|%3D|error&var-metadata=&var-jsonFields=&var-patterns=&var-lineFilterV2=&var-lineFilters=&displayedFields=[]&urlColumns=[%22Time%22,%22service_name%22,%22logger_name%22,%22detected_level%22,%22message%22,%22trace_id%22,%22stack_trace%22]&visualizationType=%22logs%22&sortOrder=%22Descending%22&timezone=browser&prettifyLogMessage=true&var-all-fields=&wrapLogMessage=true")
    }
}

fn format_and_skew(
    ts_str: &str,
    format: &str,
    skew_minutes: i64,
) -> anyhow::Result<(String, String)> {
    use chrono::{DateTime, Duration, Utc};
    use urlencoding::encode;

    match DateTime::parse_from_rfc3339(ts_str) {
        Ok(dt) => {
            let dt_utc = dt.with_timezone(&Utc);
            let from_dt = dt_utc - Duration::minutes(skew_minutes);
            let to_dt = dt_utc + Duration::minutes(skew_minutes);
            let from_fmt = from_dt.format(format).to_string();
            let to_fmt = to_dt.format(format).to_string();
            Ok((encode(&from_fmt).to_string(), encode(&to_fmt).to_string()))
        }
        Err(e) => {
            Err(anyhow::anyhow!("Failed to parse timestamp '{}'.", e))
        }
    }
}


