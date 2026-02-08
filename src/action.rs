use alloy::primitives::{Address, TxHash, U256};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct TargetTransaction {
    pub tx_hash: TxHash,
    pub from: Address,
    pub value: U256,
    pub method: String,
    #[allow(dead_code)]
    pub amount_out_min: U256,
    pub path: Vec<Address>,
    pub to: Address,
    #[allow(dead_code)]
    pub deadline: U256,
    pub detected_at: u64,
}

#[async_trait]
pub trait SniperAction: Send + Sync {
    async fn execute(&self, tx: &TargetTransaction) -> Result<()>;
}

pub struct ConsoleLogger;

impl ConsoleLogger {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SniperAction for ConsoleLogger {
    async fn execute(&self, tx: &TargetTransaction) -> Result<()> {
        let value_eth = format_wei_to_eth(tx.value);

        let path_display = if tx.path.is_empty() {
            "N/A".to_string()
        } else {
            tx.path.len().to_string()
        };

        tracing::info!(
            target: "sniper",
            "HIT! Hash: {} | Value: {} ETH | Method: {} | To: {} | Path Len: {}",
            format_args!("\x1b[33m{}\x1b[0m", tx.tx_hash),
            format_args!("\x1b[32m{}\x1b[0m", value_eth),
            format_args!("\x1b[36m{}\x1b[0m", tx.method),
            format_args!("\x1b[35m{:?}\x1b[0m", tx.to),
            format_args!("\x1b[34m{}\x1b[0m", path_display)
        );

        Ok(())
    }
}

fn format_wei_to_eth(wei: U256) -> String {
    let eth_divisor = U256::from(1_000_000_000_000_000_000u128);
    let eth_whole = wei / eth_divisor;
    let eth_fraction = wei % eth_divisor;

    let fraction_scaled = eth_fraction * U256::from(10000u128) / eth_divisor;

    format!("{}.{:04}", eth_whole, fraction_scaled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_wei_to_eth() {
        let one_eth = U256::from(1_000_000_000_000_000_000u128);
        assert_eq!(format_wei_to_eth(one_eth), "1.0000");

        let half_eth = U256::from(500_000_000_000_000_000u128);
        assert_eq!(format_wei_to_eth(half_eth), "0.5000");

        let ten_quarter_eth = U256::from(10_250_000_000_000_000_000u128);
        assert_eq!(format_wei_to_eth(ten_quarter_eth), "10.2500");

        let small_eth = U256::from(100_000_000_000_000u128);
        assert_eq!(format_wei_to_eth(small_eth), "0.0001");
    }

    #[tokio::test]
    async fn test_console_logger_execute() {
        let logger = ConsoleLogger::new();

        let tx = TargetTransaction {
            tx_hash: TxHash::default(),
            value: U256::from(1_000_000_000_000_000_000u128),
            method: "swapExactETHForTokens".to_string(),
            amount_out_min: U256::from(1000u64),
            path: vec![
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
                    .parse()
                    .unwrap(),
                "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                    .parse()
                    .unwrap(),
            ],
            to: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0"
                .parse()
                .unwrap(),
            deadline: U256::from(1700000000u64),
        };

        let result = logger.execute(&tx).await;
        assert!(result.is_ok(), "ConsoleLogger should execute successfully");
    }
}
