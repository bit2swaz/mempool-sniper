use alloy::providers::{ProviderBuilder, RootProvider, WsConnect};
use alloy::pubsub::PubSubFrontend;
use anyhow::Result;

pub async fn establish_connection(url: &str) -> Result<RootProvider<PubSubFrontend>> {
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().on_ws(ws).await?;

    Ok(provider)
}
