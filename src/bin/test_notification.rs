use anyhow::{Context, Result};
use dotenvy::dotenv;
use mempool_sniper::action::{SniperAction, TargetTransaction};
use mempool_sniper::discord::DiscordClient;
use alloy::primitives::{Address, TxHash, U256};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    
    let webhook_url = env::var("DISCORD_WEBHOOK_URL")
        .expect("DISCORD_WEBHOOK_URL must be set in .env");
    
    println!("discord webhook test script");
    println!("webhook url: {}...", &webhook_url[..50]);
    println!();
    
    let client = DiscordClient::new(&webhook_url)
        .context("Failed to create Discord client")?;
    
    let fake_tx = TargetTransaction {
        tx_hash: TxHash::from([
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef,
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef,
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef,
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef,
        ]),
        from: Address::from([
            0xd8, 0xda, 0x6b, 0xf2, 0x69, 0x64, 0xaf, 0x9d,
            0x7e, 0xed, 0x9e, 0x03, 0xe5, 0x34, 0x15, 0xd3,
            0x7a, 0xa9, 0x60, 0x45,
        ]),
        to: Address::from([
            0xc0, 0x2a, 0xaa, 0x39, 0xb2, 0x23, 0xfe, 0x8d,
            0x0a, 0x0e, 0x5c, 0x4f, 0x27, 0xea, 0xd9, 0x08,
            0x3c, 0x75, 0x6c, 0xc2,
        ]),
        value: U256::from(10_500_000_000_000_000_000u128),
        method: "swapExactETHForTokens".to_string(),
        amount_out_min: U256::from(1000000000000000000u128),
        path: vec![
            Address::from([
                0xc0, 0x2a, 0xaa, 0x39, 0xb2, 0x23, 0xfe, 0x8d,
                0x0a, 0x0e, 0x5c, 0x4f, 0x27, 0xea, 0xd9, 0x08,
                0x3c, 0x75, 0x6c, 0xc2,
            ]),
            Address::from([
                0x6b, 0x17, 0x54, 0x74, 0xe8, 0x90, 0x94, 0xc4,
                0x4d, 0xa9, 0x8b, 0x95, 0x4e, 0xed, 0xea, 0xc4,
                0x95, 0x27, 0x1d, 0x0f,
            ]),
        ],
        deadline: U256::from(9999999999u64),
        detected_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    };
    
    println!("simulated whale transaction:");
    println!("   hash: {:?}", fake_tx.tx_hash);
    println!("   from: {:#x}", fake_tx.from);
    println!("   to: {:#x}", fake_tx.to);
    println!("   value: {} eth", fake_tx.value.to_string().parse::<f64>().unwrap_or(0.0) / 1e18);
    println!("   method: {}", fake_tx.method);
    println!();
    
    println!("sending discord webhook...");
    match client.execute(&fake_tx).await {
        Ok(_) => {
            println!("success! discord alert sent!");
            println!("check your discord channel for the whale alert!");
        }
        Err(e) => {
            eprintln!("failed: {}", e);
            return Err(e);
        }
    }
    
    Ok(())
}
