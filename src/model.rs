use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

#[derive(Deserialize, Debug, Clone)]
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

    pub fn logger_name(&self) -> Option<&str> {
        self.logger_name.as_deref()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref().filter(|s| !s.is_empty())
    }

    pub fn parsed_timestamp(&self) -> Option<DateTime<Utc>> {
        self.timestamp
            .as_deref()
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.with_timezone(&Utc))
    }

    /// Aggressive normalization of the message for grouping similar errors.
    /// Strips uuids, timestamps, long hex tokens, quoted strings and numbers.
    pub fn normalized_message(&self) -> String {
        normalize_message(&self.message)
    }

    /// Group key: (container, logger, fingerprint hash). Pod intentionally excluded
    /// so restarts / replicas merge into the same aggregate.
    pub fn aggregation_key(&self, container: &str) -> String {
        let normalized = self.normalized_message();
        let mut h = DefaultHasher::new();
        normalized.hash(&mut h);
        let logger = self.logger_name.as_deref().unwrap_or("");
        format!("{container}|{logger}|{:x}", h.finish())
    }
}

fn normalize_message(input: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;

    static UUID: OnceLock<Regex> = OnceLock::new();
    static TS: OnceLock<Regex> = OnceLock::new();
    static HEX: OnceLock<Regex> = OnceLock::new();
    static DQUOTE: OnceLock<Regex> = OnceLock::new();
    static SQUOTE: OnceLock<Regex> = OnceLock::new();
    static NUM: OnceLock<Regex> = OnceLock::new();
    static WS: OnceLock<Regex> = OnceLock::new();

    let uuid = UUID.get_or_init(|| {
        Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
    });
    let ts = TS.get_or_init(|| {
        Regex::new(r"(?i)\d{4}-\d{2}-\d{2}[t ]\d{2}:\d{2}:\d{2}(\.\d+)?(z|[+-]\d{2}:?\d{2})?").unwrap()
    });
    let hex = HEX.get_or_init(|| Regex::new(r"(?i)\b[0-9a-f]{16,}\b").unwrap());
    let dquote = DQUOTE.get_or_init(|| Regex::new(r#""[^"]*""#).unwrap());
    let squote = SQUOTE.get_or_init(|| Regex::new(r"'[^']*'").unwrap());
    let num = NUM.get_or_init(|| Regex::new(r"\b\d+(\.\d+)?\b").unwrap());
    let ws = WS.get_or_init(|| Regex::new(r"\s+").unwrap());

    let lower = input.to_lowercase();
    let s = uuid.replace_all(&lower, "<uuid>");
    let s = ts.replace_all(&s, "<ts>");
    let s = hex.replace_all(&s, "<hex>");
    let s = dquote.replace_all(&s, "\"<str>\"");
    let s = squote.replace_all(&s, "'<str>'");
    let s = num.replace_all(&s, "<n>");
    let s = ws.replace_all(&s, " ");

    let trimmed = s.trim();
    if trimmed.len() > 512 {
        trimmed[..512].to_string()
    } else {
        trimmed.to_string()
    }
}

/// A representative view of an aggregate, used to render a Slack message.
pub struct AlertView<'a> {
    pub sample: &'a Log,
    pub container: &'a str,
    pub count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub pods: &'a HashSet<String>,
    pub trace_ids: &'a HashSet<String>,
}

impl<'a> AlertView<'a> {
    pub fn fallback_text(&self) -> String {
        format!(
            ":code-on-fire: {} (x{}): {}",
            self.container,
            self.count,
            truncate(&self.sample.message, 200)
        )
    }

    pub fn to_blocks(&self) -> serde_json::Value {
        let cluster = crate::env("NAIS_CLUSTER_NAME");
        let mut sorted_traces: Vec<&String> =
            self.trace_ids.iter().filter(|s| !s.is_empty()).collect();
        sorted_traces.sort();
        let single_trace = sorted_traces
            .first()
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Widen window to cover whole aggregate.
        let from = self.first_seen - Duration::minutes(1);
        let to = self.last_seen + Duration::minutes(1);

        let normalized_for_filter = self.sample.normalized_message();
        let line_filter_hint = filter_hint(&self.sample.message, &normalized_for_filter);
        let grafana_log_url =
            resolve_grafana_loki(self.container, &cluster, from, to, &line_filter_hint);
        let peisen_url = resolve_peisen_url(&cluster, &single_trace, from, to);
        let team_logs_url =
            resolve_team_logs_url(self.container, &cluster, from, to, &line_filter_hint);

        let mut action_elements: Vec<serde_json::Value> = Vec::new();
        if !single_trace.is_empty() {
            let grafana_trace_url = resolve_grafana_url(&cluster, &single_trace);
            action_elements.push(json!({
                "type": "button",
                "text": { "type": "plain_text", "text": "trace :grafana:", "emoji": true },
                "url": grafana_trace_url,
                "action_id": "button-action-1"
            }));
        }
        action_elements.push(json!({
            "type": "button",
            "text": { "type": "plain_text", "text": "open logs :grafana:", "emoji": true },
            "url": grafana_log_url,
            "action_id": "button-action-2"
        }));
        action_elements.push(json!({
            "type": "button",
            "text": { "type": "plain_text", "text": "secure logs :gcp:", "emoji": true },
            "url": team_logs_url,
            "action_id": "button-action-3"
        }));
        action_elements.push(json!({
            "type": "button",
            "text": { "type": "plain_text", "text": "peisen :wood:", "emoji": true },
            "url": peisen_url,
            "action_id": "button-action-4"
        }));

        let cluster_label = match cluster.as_str() {
            "prod-gcp" => ":alert: PROD :alert:",
            _ => "DEV",
        };

        let pods_line = format_pods(self.pods);
        let traces_line = format_traces(self.trace_ids);
        let header_text = if self.count > 1 {
            format!(":code-on-fire: {} (x{})", self.container, self.count)
        } else {
            format!(":code-on-fire: {}", self.container)
        };

        let stats_text = format!(
            "count: {}   first: {}   last: {}",
            self.count,
            self.first_seen.format("%Y-%m-%d %H:%M:%S UTC"),
            self.last_seen.format("%Y-%m-%d %H:%M:%S UTC")
        );

        json!({
            "blocks": [
                {
                    "type": "header",
                    "text": {
                        "type": "plain_text",
                        "text": header_text,
                        "emoji": true
                    }
                },
                {
                    "type": "rich_text",
                    "elements": [
                        {
                            "type": "rich_text_section",
                            "elements": [
                                { "type": "text", "text": pods_line, "style": { "italic": true } }
                            ]
                        }
                    ]
                },
                {
                    "type": "section",
                    "text": { "type": "plain_text", "text": cluster_label, "emoji": true }
                },
                { "type": "divider" },
                {
                    "type": "section",
                    "text": { "type": "plain_text", "text": stats_text, "emoji": true }
                },
                {
                    "type": "section",
                    "text": {
                        "type": "plain_text",
                        "text": format!("logger: {}", self.sample.logger_name().unwrap_or("log")),
                        "emoji": true
                    }
                },
                {
                    "type": "rich_text",
                    "elements": [
                        {
                            "type": "rich_text_preformatted",
                            "elements": [
                                { "type": "text", "text": self.sample.message.clone() }
                            ]
                        }
                    ]
                },
                { "type": "divider" },
                {
                    "type": "section",
                    "text": { "type": "plain_text", "text": traces_line, "emoji": true }
                },
                {
                    "type": "actions",
                    "elements": action_elements
                }
            ]
        })["blocks"]
            .clone()
    }
}

fn format_pods(pods: &HashSet<String>) -> String {
    let mut sorted: Vec<&String> = pods.iter().collect();
    sorted.sort();
    match sorted.len() {
        0 => String::new(),
        1..=3 => sorted
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        n => format!("{}, {}, … (+{} more)", sorted[0], sorted[1], n - 2),
    }
}

fn format_traces(trace_ids: &HashSet<String>) -> String {
    let mut sorted: Vec<&String> = trace_ids.iter().filter(|s| !s.is_empty()).collect();
    sorted.sort();
    match sorted.len() {
        0 => "trace_id: -".to_string(),
        1 => format!("trace_id: {}", sorted[0]),
        2..=3 => format!(
            "trace_ids: {}",
            sorted
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        n => format!("trace_ids: {} distinct ({} shown)", n, sorted[0]),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Pull a stable substring from the original message to use as a log search filter,
/// favouring portions that the normalizer did NOT replace with placeholders.
fn filter_hint(original: &str, normalized: &str) -> String {
    // If normalization mostly preserved the prefix, use that. Otherwise fall back
    // to the longest run of letters from the original.
    let first_token: String = normalized
        .split_whitespace()
        .take_while(|t| !t.starts_with('<'))
        .collect::<Vec<_>>()
        .join(" ");

    let candidate = if first_token.len() >= 8 {
        first_token
    } else {
        original
            .split(|c: char| !c.is_alphabetic() && c != ' ')
            .max_by_key(|s| s.len())
            .unwrap_or("")
            .trim()
            .to_string()
    };

    candidate.chars().take(60).collect::<String>().trim().to_string()
}

fn resolve_team_logs_url(
    container: &str,
    cluster: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter_hint: &str,
) -> String {
    use urlencoding::encode;

    let host = "https://console.cloud.google.com";
    let start = from.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let end = to.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let time_range = format!("{start}%2F{end}");

    let namespace = crate::env("NAIS_NAMESPACE");
    let severity = "ERROR";

    let mut query_lines = vec![
        "resource.type=\"k8s_container\"".to_string(),
        format!("resource.labels.container_name=\"{container}\""),
        format!("resource.labels.namespace_name=\"{namespace}\""),
        format!("severity>={severity}"),
    ];
    if !filter_hint.is_empty() {
        query_lines.push(format!("textPayload:\"{filter_hint}\""));
    }
    let query = encode(&query_lines.join("\n")).to_string();

    let project = match cluster {
        "prod-gcp" => "helved-prod-119e",
        _ => "helved-dev-9e3f",
    };

    format!("{host}/logs/query;query={query};timeRange={time_range}?project={project}")
}

fn resolve_grafana_url(cluster: &str, trace_id: &str) -> String {
    let datasource = match cluster {
        "prod-gcp" => "22P8A28344D07741F8D",
        _ => "22P95CC91DC09CABFC8",
    };
    let host = "https://grafana.nav.cloud.nais.io";
    format!("{host}/explore?schemaVersion=1&panes=%7B%22trace%22%3A%7B%22datasource%22%3A%{datasource}%22%2C%22queries%22%3A%5B%7B%22queryType%22%3A%22traceql%22%2C%22query%22%3A%22{trace_id}%22%7D%5D%7D%7D")
}

fn resolve_peisen_url(
    cluster: &str,
    trace_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> String {
    let host = match cluster {
        "prod-gcp" => "https://peisen.intern.nav.no",
        _ => "https://peisen.intern.dev.nav.no",
    };

    let fom = from.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let tom = to.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let fom_enc = urlencoding::encode(&fom);
    let tom_enc = urlencoding::encode(&tom);

    if trace_id.is_empty() {
        format!("{host}/kafka?fom={fom_enc}&tom={tom_enc}")
    } else {
        format!("{host}/kafka?fom={fom_enc}&tom={tom_enc}&trace_id={trace_id}")
    }
}

fn resolve_grafana_loki(
    container: &str,
    cluster: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    filter_hint: &str,
) -> String {
    let host = "https://grafana.nav.cloud.nais.io";
    let var_ds = match cluster {
        "prod-gcp" => "PD969E40991D5C4A8",
        _ => "P7BE696147D279490",
    };
    let from_fmt = urlencoding::encode(&from.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()).to_string();
    let to_fmt = urlencoding::encode(&to.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()).to_string();

    let line_filter = if filter_hint.is_empty() {
        String::new()
    } else {
        // Loki Explore expects: caseSensitive|operator|value, where operator |= means "contains".
        format!(
            "&var-lineFilterV2=caseInsensitive%2C1%7C__gfp__%3D%7C{}",
            urlencoding::encode(filter_hint)
        )
    };

    format!("{host}/a/grafana-lokiexplore-app/explore/service/{container}/logs?from={from_fmt}&to={to_fmt}&var-ds={var_ds}&var-filters=service_name|%3D|{container}&patterns=[]&var-lineFormat=&var-fields=&var-levels=detected_level|%3D|Error&var-levels=detected_level|%3D|error&var-metadata=&var-jsonFields=&var-patterns={line_filter}&displayedFields=[]&urlColumns=[%22Time%22,%22service_name%22,%22logger_name%22,%22detected_level%22,%22message%22,%22trace_id%22,%22stack_trace%22]&visualizationType=%22logs%22&sortOrder=%22Descending%22&timezone=browser&prettifyLogMessage=true&var-all-fields=&wrapLogMessage=true")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_uuid_ts_hex_numbers_quotes() {
        let msg = "Failed to process behandling 7c3e4d12-a1b2-4c3d-9e8f-1234567890ab at 2025-05-19T08:00:00.123Z status=500 trace=deadbeefcafebabe1234567890abcdef msg=\"oops\"";
        let n = normalize_message(msg);
        assert!(n.contains("<uuid>"));
        assert!(n.contains("<ts>"));
        assert!(n.contains("<hex>"));
        assert!(n.contains("<n>"));
        assert!(n.contains("\"<str>\""));
    }

    #[test]
    fn aggregation_key_stable_across_volatile_tokens() {
        let make = |m: &str| Log {
            level: "ERROR".into(),
            timestamp: None,
            logger_name: Some("foo".into()),
            message: m.to_string(),
            trace_id: None,
            span_id: None,
            hostname: None,
        };
        let a = make("NPE in handleEvent(eventId=12345678-1234-1234-1234-123456789012)");
        let b = make("NPE in handleEvent(eventId=87654321-4321-4321-4321-210987654321)");
        assert_eq!(a.aggregation_key("c1"), b.aggregation_key("c1"));
    }
}
