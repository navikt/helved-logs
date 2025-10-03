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
    ) -> anyhow::Result<reqwest::Response>{
        let alert = log.to_slack_alert(container_name, pod_name);
        println!("alert: {}", &alert.to_string());
        let res = reqwest::Client::new() 
            .post(&self.webhook)
            .json(&alert)
            .send()
            .await?;

        Ok(res)
    }
}

