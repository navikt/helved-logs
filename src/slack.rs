use anyhow::{anyhow, Result};
use serde::Deserialize;

const SLACK_POST_URL: &str = "https://slack.com/api/chat.postMessage";
const SLACK_UPDATE_URL: &str = "https://slack.com/api/chat.update";

#[derive(Clone, Debug)]
pub struct PostedMessage {
    pub channel: String,
    pub ts: String,
}

#[derive(Deserialize)]
struct SlackResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    channel: Option<String>,
}

pub struct Slack {
    token: String,
    channel: String,
    http: reqwest::Client,
}

impl Default for Slack {
    fn default() -> Self {
        Slack {
            token: crate::env("SLACK_BOT_TOKEN"),
            channel: crate::env("SLACK_CHANNEL"),
            http: reqwest::Client::new(),
        }
    }
}

impl Slack {
    pub async fn post(
        &self,
        blocks: serde_json::Value,
        fallback_text: &str,
    ) -> Result<PostedMessage> {
        let body = serde_json::json!({
            "channel": self.channel,
            "text": fallback_text,
            "blocks": blocks,
        });

        let resp: SlackResponse = self
            .http
            .post(SLACK_POST_URL)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            return Err(anyhow!(
                "slack chat.postMessage failed: {}",
                resp.error.unwrap_or_else(|| "unknown error".into())
            ));
        }

        Ok(PostedMessage {
            channel: resp.channel.unwrap_or_else(|| self.channel.clone()),
            ts: resp
                .ts
                .ok_or_else(|| anyhow!("slack postMessage returned ok but no ts"))?,
        })
    }

    pub async fn update(
        &self,
        posted: &PostedMessage,
        blocks: serde_json::Value,
        fallback_text: &str,
    ) -> Result<()> {
        let body = serde_json::json!({
            "channel": posted.channel,
            "ts": posted.ts,
            "text": fallback_text,
            "blocks": blocks,
        });

        let resp: SlackResponse = self
            .http
            .post(SLACK_UPDATE_URL)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            return Err(anyhow!(
                "slack chat.update failed: {}",
                resp.error.unwrap_or_else(|| "unknown error".into())
            ));
        }

        Ok(())
    }
}
