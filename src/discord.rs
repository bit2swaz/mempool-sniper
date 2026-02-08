use crate::action::{SniperAction, TargetTransaction};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

struct RateLimiter {
    last_request: Instant,
    min_interval: Duration,
}

impl RateLimiter {
    fn new(requests_per_minute: u32) -> Self {
        let min_interval = Duration::from_secs(60) / requests_per_minute;
        Self {
            last_request: Instant::now() - min_interval,
            min_interval,
        }
    }

    async fn acquire(&mut self) {
        let elapsed = self.last_request.elapsed();
        if elapsed < self.min_interval {
            let wait_time = self.min_interval - elapsed;
            tokio::time::sleep(wait_time).await;
        }
        self.last_request = Instant::now();
    }
}

pub struct DiscordClient {
    webhook_url: String,
    client: reqwest::Client,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl DiscordClient {
    pub fn new(webhook_url: &str) -> Result<Self> {
        tracing::info!("discord webhook client initialized");
        
        Ok(Self {
            webhook_url: webhook_url.to_string(),
            client: reqwest::Client::new(),
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new(25))),
            margin
        })
    }

    async fn send_alert(&self, tx: &TargetTransaction) -> Result<()> {
        self.rate_limiter.lock().await.acquire().await;

        let eth_value = tx.value.to_string().parse::<f64>().unwrap_or(0.0) / 1e18;
        
        let etherscan_link = format!("https://sepolia.etherscan.io/tx/{:?}", tx.tx_hash);
        let tx_hash_short = format!("{:?}", tx.tx_hash);
        let tx_display = format!("[{}...{}]({})", 
            &tx_hash_short[0..10], 
            &tx_hash_short[tx_hash_short.len()-8..],
            etherscan_link
        );
        
        let to_display = format!("{:#x}", tx.to);

        let payload = json!({
            "embeds": [{
                "title": "transaction detected",
                "color": 0x00ff00,
                "fields": [
                    {
                        "name": "value",
                        "value": format!("**{:.4} eth**", eth_value),
                        "inline": true
                    },
                    {
                        "name": "method",
                        "value": format!("`{}`", tx.method),
                        "inline": true
                    },
                    {
                        "name": "transaction",
                        "value": tx_display,
                        "inline": false
                    },
                    {
                        "name": "from",
                        "value": format!("`{:#x}`", tx.from),
                        "inline": true
                    },
                    {
                        "name": "to",
                        "value": format!("`{}`", to_display),
                        "inline": true
                    },
                    {
                        "name": "detected",
                        "value": format!("<t:{}:R>", tx.detected_at / 1000),
                        "inline": false
                    }
                ],
                "footer": {
                    "text": "mempool sniper - real-time monitor"
                },
                "timestamp": chrono::Utc::now().to_rfc3339()
            }]
        });

        let response = self
            .client
            .post(&self.webhook_url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("failed to send discord webhook")?;

        if response.status().is_success() {
            tracing::info!("discord alert sent for tx {:?}", tx.tx_hash);
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "discord webhook failed: {} - {}",
                status,
                error_text
            )
        }
    }
}

#[async_trait]
impl SniperAction for DiscordClient {
    async fn execute(&self, tx: &TargetTransaction) -> Result<()> {
        tracing::info!(
            "target detected: {:?} | {:.4} eth | {}",
            tx.tx_hash,
            tx.value.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
            tx.method
        );

        if let Err(e) = self.send_alert(tx).await {
            tracing::error!("failed to send discord alert: {}", e);
        }

        Ok(())
    }
}
