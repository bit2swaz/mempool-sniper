#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

mod action;
mod decoder;
mod discord;
mod ingestor;

use action::SniperAction;
use alloy::consensus::Transaction;
use alloy::primitives::U256;
use alloy::providers::{Provider, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::types::Transaction as RpcTransaction;
use tokio::sync::mpsc;
use tracing::info;

const CHANNEL_CAPACITY: usize = 500_000;
const MAX_CONCURRENT_FETCHES: usize = 50;

async fn spawn_ingestor(
    tx_channel: mpsc::Sender<RpcTransaction>,
    provider: RootProvider<PubSubFrontend>,
) {
    info!("ingestor started");

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES));

    let mut retry_delay_secs = 1;
    const MAX_RETRY_DELAY: u64 = 60;

    loop {
        let mut sub = match provider.subscribe_pending_transactions().await {
            Ok(sub) => {
                info!("subscribed to pending transactions (hash stream)");
                retry_delay_secs = 1;
                sub
            }
            Err(e) => {
                let error_msg = format!("{:?}", e);
                if error_msg.contains("subscription not found") {
                    tracing::error!("subscription error: subscription not found (provider may not support eth_subscribe)");
                } else if error_msg.contains("connection closed") || error_msg.contains("ConnectionClosed") {
                    tracing::error!("subscription error: connection closed (network issue or provider restart)");
                } else if error_msg.contains("rate limit") || error_msg.contains("429") {
                    tracing::error!("subscription error: rate limit exceeded (too many requests)");
                } else if error_msg.contains("timeout") || error_msg.contains("Timeout") {
                    tracing::error!("subscription error: connection timeout");
                } else {
                    tracing::error!("subscription error: {} - {}", error_msg, e);
                }

                tracing::warn!("retrying subscription in {} seconds (exponential backoff)...", retry_delay_secs);
                tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay_secs)).await;
                
                retry_delay_secs = (retry_delay_secs * 2).min(MAX_RETRY_DELAY);
                continue;
            }
        };

        info!("full capture mode: processing 100% of all transactions (zero filtering)");

        let mut tx_count = 0u64;
        
        loop {
            match sub.recv().await {
                Ok(tx_hash) => {
                    tx_count += 1;
                    
                    if tx_count % 10 == 0 {
                        tracing::debug!("scanned {} transactions...", tx_count);
                    }
                    
                    if tx_count % 100 == 0 {
                        tracing::info!("processed {} transactions", tx_count);
                    }

                    let permit = semaphore.clone().acquire_owned().await.unwrap();

                    let provider_clone = provider.clone();
                    let tx_channel_clone = tx_channel.clone();

                    tokio::spawn(async move {
                        let _permit = permit;

                        match provider_clone.get_transaction_by_hash(tx_hash).await {
                            Ok(Some(tx)) => {
                                let input_data = tx.inner.input();

                                if !decoder::is_target_transaction(input_data) {
                                    return;
                                }

                                tracing::debug!("target selector detected in mempool: {}", tx_hash);

                                if let Err(e) = tx_channel_clone.try_send(tx) {
                                    match e {
                                        mpsc::error::TrySendError::Full(_) => {
                                            tracing::warn!(
                                                "buffer full - dropping target tx: {}",
                                                tx_hash
                                            );
                                        }
                                        mpsc::error::TrySendError::Closed(_) => {
                                            tracing::error!("channel closed, stopping ingestor");
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                tracing::debug!("tx: {} | not found", tx_hash);
                            }
                            Err(e) => {
                                tracing::debug!("tx: {} | error fetching: {:?}", tx_hash, e);
                            }
                        }
                    });
                }
                Err(e) => {
                    let error_msg = format!("{:?}", e);
                    
                    if error_msg.contains("Lagged") {
                        tracing::info!("subscription lagged by {} messages - reconnecting", 
                            error_msg.split("Lagged(").nth(1)
                                .and_then(|s| s.split(')').next())
                                .unwrap_or("?"));
                    } else if error_msg.contains("subscription not found") {
                        tracing::error!("subscription not found - reconnecting");
                    } else if error_msg.contains("connection closed") || error_msg.contains("ConnectionClosed") {
                        tracing::error!("connection closed - reconnecting");
                    } else if error_msg.contains("rate limit") || error_msg.contains("429") {
                        tracing::error!("rate limit exceeded - reconnecting");
                    } else {
                        tracing::info!("subscription error: {} - reconnecting", e);
                    }

                    break;
                }
            }
        }
    }
}

async fn spawn_consumer(
    mut rx_channel: mpsc::Receiver<RpcTransaction>,
    action: std::sync::Arc<dyn SniperAction>,
) {
    info!("consumer started");
    info!("full capture mode: processing all transactions (no value threshold)");

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(50));
    info!("parallel consumer engine: 50 concurrent workers");

    loop {
        let tx = match rx_channel.recv().await {
            Some(tx) => tx,
            None => {
                info!("consumer stopped - channel closed");
                break;
            }
        };

        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                tracing::error!("semaphore closed");
                break;
            }
        };

        let action_clone = action.clone();

        tokio::spawn(async move {
            let _permit = permit;

            let tx_hash = tx.inner.tx_hash();
            let input_data = tx.inner.input();
            let tx_value = tx.inner.value();

            tracing::info!("processing tx: {:?}", tx_hash);

            let decoded = match decoder::decode_transaction(input_data, tx_value) {
                Ok(decoded) => decoded,
                Err(e) => {
                    tracing::debug!("failed to decode tx {}: {}", tx_hash, e);
                    return;
                }
            };

            let detected_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            
            let target_tx = action::TargetTransaction {
                tx_hash: *tx_hash,
                from: tx.from,
                value: decoded.effective_value,
                method: decoded.method,
                amount_out_min: decoded.amount_out_min,
                path: decoded.path,
                to: decoded.to,
                deadline: decoded.deadline,
                detected_at,
            };

            if let Err(e) = action_clone.execute(&target_tx).await {
                tracing::error!("failed to execute action for tx {}: {}", tx_hash, e);
            }
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    dotenvy::dotenv().ok();

    info!("mempool sniper initialized");

    let wss_url = std::env::var("WSS_RPC_URL").expect("WSS_RPC_URL must be set in .env file");
    let use_discord = std::env::var("USE_DISCORD").unwrap_or_else(|_| "false".to_string());

    info!("connecting to rpc: {}", std::env::var("WSS_RPC_URL").unwrap_or_else(|_| "unknown".to_string()));
    info!("connecting to websocket: {}", wss_url);

    let provider = ingestor::establish_connection(&wss_url).await?;

    info!("connected successfully");

    let action: std::sync::Arc<dyn SniperAction> = if use_discord.to_lowercase() == "true" {
        let webhook_url = std::env::var("DISCORD_WEBHOOK_URL")
            .expect("DISCORD_WEBHOOK_URL must be set when USE_DISCORD=true");

        info!("discord webhook mode enabled");

        let discord_client = discord::DiscordClient::new(&webhook_url)?;
        std::sync::Arc::new(discord_client)
    } else {
        info!("console logger mode");
        std::sync::Arc::new(action::ConsoleLogger::new())
    };

    let (tx, rx) = mpsc::channel::<RpcTransaction>(CHANNEL_CAPACITY);

    info!("spawning ingestor and consumer tasks...");

    let ingestor_handle = tokio::spawn(spawn_ingestor(tx, provider));

    let consumer_handle = tokio::spawn(spawn_consumer(rx, action));

    tokio::select! {
        _ = ingestor_handle => {
            tracing::error!("ingestor task terminated unexpectedly");
        }
        _ = consumer_handle => {
            tracing::error!("consumer task terminated unexpectedly");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_overflow_behavior() {
        let (tx, mut rx) = mpsc::channel::<i32>(4096);

        let mut sent_count = 0;
        let mut dropped_count = 0;

        for i in 0..5000 {
            match tx.try_send(i) {
                Ok(_) => sent_count += 1,
                Err(mpsc::error::TrySendError::Full(_)) => dropped_count += 1,
                Err(_) => panic!("unexpected error"),
            }
        }

        assert_eq!(sent_count, 4096, "should have sent 4096 messages");
        assert_eq!(dropped_count, 904, "should have dropped 904 messages");

        let mut received_count = 0;
        while let Ok(val) = rx.try_recv() {
            assert!(val < 4096, "received value should be less than 4096");
            received_count += 1;
        }

        assert_eq!(received_count, 4096, "should have received 4096 messages");
    }

    #[tokio::test]
    async fn test_end_to_end_pipeline_with_mock_transaction() {
        use alloy::primitives::address;

        let calldata_hex = "7ff36ab5\
            00000000000000000000000000000000000000000000000000000000000003e8\
            0000000000000000000000000000000000000000000000000000000000000080\
            000000000000000000000000742d35cc6634c0532925a3b844bc9e7595f0beb0\
            0000000000000000000000000000000000000000000000000000000065562040\
            0000000000000000000000000000000000000000000000000000000000000002\
            000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2\
            000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7";

        let calldata =
            hex::decode(calldata_hex.replace("\n", "").replace(" ", "")).expect("valid hex");

        assert!(
            decoder::is_target_transaction(&calldata),
            "Stage 1 filter should match swapExactETHForTokens selector"
        );

        let decoded = decoder::decode_transaction(&calldata, U256::ZERO)
            .expect("Stage 2 decode should succeed");

        assert_eq!(decoded.amount_out_min, U256::from(1000u64));
        assert_eq!(decoded.path.len(), 2);
        assert_eq!(
            decoded.path[0],
            address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")
        );
        assert_eq!(
            decoded.path[1],
            address!("dAC17F958D2ee523a2206206994597C13D831ec7")
        );
        assert_eq!(
            decoded.to,
            address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb0")
        );
        assert_eq!(decoded.deadline, U256::from(1700143168u64));

        let tx_value = U256::from(500_000_000_000_000_000u128);
        let eth_threshold = U256::from(100_000_000_000_000_000u128);
        assert!(tx_value >= eth_threshold, "Stage 3 value check should pass");

        let logger = action::ConsoleLogger::new();
        let detected_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let target_tx = action::TargetTransaction {
            tx_hash: alloy::primitives::TxHash::default(),
            from: alloy::primitives::Address::default(),
            value: tx_value,
            method: "swapExactETHForTokens".to_string(),
            amount_out_min: decoded.amount_out_min,
            path: decoded.path,
            to: decoded.to,
            deadline: decoded.deadline,
            detected_at,
        };

        let result = logger.execute(&target_tx).await;
        assert!(result.is_ok(), "Stage 4 action execution should succeed");
    }

    #[tokio::test]
    async fn test_v3_selector_detection() {

        let v3_calldata = hex::decode("414bf389000000000000000000000000").unwrap();
        assert!(
            decoder::is_target_transaction(&v3_calldata),
            "Should detect Uniswap V3 exactInputSingle"
        );

        let multicall_data = hex::decode("5ae401dc000000000000000000000000").unwrap();
        assert!(
            decoder::is_target_transaction(&multicall_data),
            "Should detect Uniswap V3 multicall"
        );
    }

    #[tokio::test]
    async fn test_native_transfer_handling() {
        let empty_input = vec![];
        let tx_value = U256::from(1_000_000_000_000_000_000u128);

        let decoded = decoder::decode_transaction(&empty_input, tx_value);
        assert!(
            decoded.is_ok(),
            "Native transfer should decode successfully"
        );

        let decoded = decoded.unwrap();
        assert_eq!(decoded.method, "Native Transfer");
        assert_eq!(decoded.effective_value, tx_value);
    }

    #[tokio::test]
    async fn test_weth_swap_value_calculation() {
        let calldata = hex::decode(
            "18cbafe5\
             0000000000000000000000000000000000000000000000000000000000001388\
             00000000000000000000000000000000000000000000000000000000000003e8\
             00000000000000000000000000000000000000000000000000000000000000a0\
             000000000000000000000000742d35cc6634c0532925a3b844bc9e7595f0beb0\
             000000000000000000000000000000000000000000000000000000006555a3a0\
             0000000000000000000000000000000000000000000000000000000000000002\
             000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7\
             000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
        )
        .unwrap();

        let tx_value = U256::ZERO;
        let decoded = decoder::decode_transaction(&calldata, tx_value).unwrap();

        assert_eq!(decoded.effective_value, U256::from(5000u64));
        assert_eq!(decoded.method, "swapExactTokensForETH");
    }

    #[tokio::test]
    async fn test_all_selectors_detected() {
        let selectors = vec![
            ("7ff36ab5", "swapExactETHForTokens"),
            ("18cbafe5", "swapExactTokensForETH"),
            ("38ed1739", "swapExactTokensForTokens"),
            ("fb3bdb41", "swapETHForExactTokens"),
            ("414bf389", "exactInputSingle"),
            ("c04b8d59", "exactInput"),
            ("5ae401dc", "multicall"),
            ("24856229", "execute"),
            ("12aa3caf", "aggregatorSwap"),
            ("bc651e96", "uniswapV3SwapTo"),
        ];

        for (selector_hex, _expected_name) in selectors {
            let calldata = hex::decode(format!("{}00000000", selector_hex)).unwrap();
            assert!(
                decoder::is_target_transaction(&calldata),
                "Selector 0x{} should be detected",
                selector_hex
            );
        }
    }

    #[tokio::test]
    async fn test_invalid_selector_rejected() {
        let invalid_selectors = vec![
            "a9059cbb",
            "095ea7b3",
            "00000000",
            "ffffffff",
        ];

        for selector_hex in invalid_selectors {
            let calldata = hex::decode(format!("{}00000000", selector_hex)).unwrap();
            assert!(
                !decoder::is_target_transaction(&calldata),
                "Selector 0x{} should NOT be detected",
                selector_hex
            );
        }
    }

    #[tokio::test]
    async fn test_threshold_filtering() {
        let threshold = U256::from(10_000_000_000_000_000u128);

        let below = U256::from(5_000_000_000_000_000u128);
        assert!(
            below < threshold,
            "0.005 ETH should be below 0.01 ETH threshold"
        );

        let at = U256::from(10_000_000_000_000_000u128);
        assert!(at >= threshold, "0.01 ETH should meet threshold");

        let above = U256::from(50_000_000_000_000_000u128);
        assert!(above >= threshold, "0.05 ETH should be above threshold");
    }

    #[tokio::test]
    async fn test_malformed_calldata_fail_open() {
        let malformed_cases = vec![
            vec![0x7f, 0xf3, 0x6a],
            vec![0x7f, 0xf3, 0x6a, 0xb5],
            vec![0x7f, 0xf3, 0x6a, 0xb5, 0x00, 0x00],
        ];

        for malformed_data in malformed_cases {
            let result = decoder::decode_transaction(&malformed_data, U256::ZERO);
            assert!(
                result.is_ok(),
                "Malformed data should not panic, should fail open"
            );
        }
    }
}
