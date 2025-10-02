use std::env;

use crate::model::Log;

pub struct Slack {
    webhook: String,
}

impl Default for Slack {
    fn default() -> Self {
        Slack {
            // webhook: env::var("apiUrl").expect("secret 'slack-webhook' not mounted"),
            webhook: env::var("apiUrl").unwrap_or("dummy-webhook".into()),
        }
    }
}

impl Slack {
    pub async fn send(&self, log: Log, container_name: String, pod_name: String) -> anyhow::Result<()>{
        let msg = log.to_slack_alert(container_name, pod_name);
        let echo_json: serde_json::Value = reqwest::Client::new()
            .post(&self.webhook)
            .json(&msg)
            .send()
            .await?
            .json()
            .await?;
        println!("{echo_json:#?}");
        Ok(())
    }
}

