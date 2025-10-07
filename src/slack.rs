use anyhow::Result;
use anyhow::Context;
use crate::model::Log;

pub struct Slack {
    webhook: String,
}

impl Default for Slack {
    fn default() -> Self {
        Slack {
            webhook: crate::env("apiUrl"),
        }
    }
}

impl Slack {
    pub async fn send(
        &self,
        log: Log,
        container_name: String,
        pod_name: String,
    ) -> Result<()> {
        let alert = log.to_slack_alert(container_name, pod_name);

        // match serde_json::to_string_pretty(&alert) {
        //     Ok(payload) => {
        //         log::info!("--- BEGIN SLACK PAYLOAD ---");
        //         log::info!("{}", payload);
        //         log::info!("--- END SLACK PAYLOAD ---");
        //     },
        //     Err(e) => {
        //         return Err(anyhow::anyhow!("Failed to serialize slack alert: {}", e));
        //     }
        // }
        let res = reqwest::Client::new() 
            .post(&self.webhook)
            .json(&alert)
            .send()
            .await?;

        let status = res.status();
        if status.is_server_error() || status.is_client_error() {
            let error_body = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("slack returned http error {}: {}", status, error_body));
        }

        let body = res.text().await.context("failed to read slack response body")?;
        if body != "ok" {
            return Err(anyhow::anyhow!("slack api rejected payload: {}", body));
        }

        log::info!("slack message sent and validated succesfully.");
        Ok(())
    }
}

