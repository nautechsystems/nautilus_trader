// Example to test the Coinbase HTTP client
use nautilus_coinbase::{
    config::CoinbaseHttpConfig,
    http::client::CoinbaseHttpClient,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let separator = "=".repeat(70);
    println!("{}", separator);
    println!("Coinbase HTTP Client Test");
    println!("{}", separator);

    // Get credentials from environment
    let api_key = std::env::var("COINBASE_API_KEY")
        .expect("COINBASE_API_KEY environment variable not set");
    let api_secret = std::env::var("COINBASE_API_SECRET")
        .expect("COINBASE_API_SECRET environment variable not set");

    println!("\n1. Creating HTTP client...");
    println!("   API Key: {}...", &api_key[..50.min(api_key.len())]);
    
    let config = CoinbaseHttpConfig {
        api_key,
        api_secret,
        base_url: None,
        timeout_secs: Some(30),
    };

    let client = CoinbaseHttpClient::new(config)?;
    println!("   ✓ Client created successfully");

    println!("\n2. Fetching available products...");
    match client.list_products().await {
        Ok(response) => {
            println!("   ✓ Successfully fetched products!");
            println!("   Total products: {}", response.products.len());
            println!("\n   First 5 products:");
            for (i, product) in response.products.iter().take(5).enumerate() {
                println!("     {}. {}", i + 1, product.product_id);
            }
        }
        Err(e) => {
            println!("   ✗ Failed to fetch products: {}", e);
            println!("\n   This could be due to:");
            println!("   - Invalid API credentials");
            println!("   - Network connectivity issues");
            println!("   - Coinbase API being down");
            return Err(e);
        }
    }

    println!("\n3. Fetching account information...");
    match client.list_accounts().await {
        Ok(response) => {
            println!("   ✓ Successfully fetched accounts!");
            println!("   Total accounts: {}", response.accounts.len());
            println!("\n   Accounts with balance:");
            let mut total_usd = 0.0;
            for account in &response.accounts {
                let balance = &account.available_balance;
                if let Ok(bal) = balance.value.parse::<f64>() {
                    if bal > 0.0 {
                        println!("     {} - Available: {} {}",
                            account.currency, balance.value, balance.currency);

                        // Track USD/USDC balances
                        if account.currency == "USD" || account.currency == "USDC" {
                            total_usd += bal;
                        }
                    }
                }
            }
            if total_usd > 0.0 {
                println!("\n   Total USD/USDC: ${:.2}", total_usd);
            }
        }
        Err(e) => {
            println!("   ✗ Failed to fetch accounts: {}", e);
        }
    }

    println!("\n4. Fetching market data for BTC-USD...");
    match client.get_product("BTC-USD").await {
        Ok(product) => {
            println!("   ✓ Successfully fetched BTC-USD market data!");
            println!("\n   Product: {}", product.product_id);
            if let Some(price) = &product.price {
                println!("   Current Price: ${}", price);
            }
            if let Some(volume) = &product.volume_24h {
                println!("   24h Volume: {} BTC", volume);
            }
            if let Some(price_change) = &product.price_percentage_change_24h {
                println!("   24h Price Change: {}%", price_change);
            }
            println!("   Min Order Size: ${}", product.quote_min_size);
            println!("   Max Order Size: ${}", product.quote_max_size);
            println!("   Status: {}", product.status);
        }
        Err(e) => {
            println!("   ✗ Failed to fetch product: {}", e);
        }
    }

    println!("\n5. Fetching market data for ETH-USD...");
    match client.get_product("ETH-USD").await {
        Ok(product) => {
            println!("   ✓ Successfully fetched ETH-USD market data!");
            println!("\n   Product: {}", product.product_id);
            if let Some(price) = &product.price {
                println!("   Current Price: ${}", price);
            }
            if let Some(volume) = &product.volume_24h {
                println!("   24h Volume: {} ETH", volume);
            }
            if let Some(price_change) = &product.price_percentage_change_24h {
                println!("   24h Price Change: {}%", price_change);
            }
        }
        Err(e) => {
            println!("   ✗ Failed to fetch product: {}", e);
        }
    }

    println!("\n6. Fetching market data for SOL-USD...");
    match client.get_product("SOL-USD").await {
        Ok(product) => {
            println!("   ✓ Successfully fetched SOL-USD market data!");
            println!("\n   Product: {}", product.product_id);
            if let Some(price) = &product.price {
                println!("   Current Price: ${}", price);
            }
            if let Some(volume) = &product.volume_24h {
                println!("   24h Volume: {} SOL", volume);
            }
            if let Some(price_change) = &product.price_percentage_change_24h {
                println!("   24h Price Change: {}%", price_change);
            }
            println!("   Trading Status: {}", product.status);
            println!("   Trading Disabled: {}", product.trading_disabled);
        }
        Err(e) => {
            println!("   ✗ Failed to fetch product: {}", e);
        }
    }

    println!("\n7. Calculating portfolio value...");
    let mut total_value_usd = 0.0;
    let mut crypto_holdings = Vec::new();

    match client.list_accounts().await {
        Ok(response) => {
            for account in &response.accounts {
                let balance = &account.available_balance;
                if let Ok(bal) = balance.value.parse::<f64>() {
                    if bal > 0.0 && account.currency != "USD" && account.currency != "USDC" {
                        crypto_holdings.push((account.currency.clone(), bal));
                    }
                }
            }

            println!("   ✓ Found {} crypto holdings", crypto_holdings.len());
            println!("\n   Fetching current prices...");

            for (currency, amount) in &crypto_holdings {
                let product_id = format!("{}-USD", currency);
                if let Ok(product) = client.get_product(&product_id).await {
                    if let Some(price_str) = &product.price {
                        if let Ok(price) = price_str.parse::<f64>() {
                            let value = amount * price;
                            total_value_usd += value;
                            println!("     {} {}: ${:.2} (@ ${}/{})",
                                amount, currency, value, price, currency);
                        }
                    }
                }
            }

            println!("\n   💰 Total Portfolio Value: ${:.2}", total_value_usd);
        }
        Err(e) => {
            println!("   ✗ Failed to calculate portfolio value: {}", e);
        }
    }

    println!("\n8. Market Summary (Top 5 by volume)...");
    match client.list_products().await {
        Ok(response) => {
            let mut products_with_volume: Vec<_> = response.products.iter()
                .filter_map(|p| {
                    p.volume_24h.as_ref().and_then(|v| {
                        v.parse::<f64>().ok().map(|vol| (p, vol))
                    })
                })
                .collect();

            products_with_volume.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            println!("   ✓ Top 5 products by 24h volume:");
            println!();
            for (i, (product, volume)) in products_with_volume.iter().take(5).enumerate() {
                let price = product.price.as_ref().map(|s| s.as_str()).unwrap_or("N/A");
                let change = product.price_percentage_change_24h.as_ref().map(|s| s.as_str()).unwrap_or("N/A");
                println!("     {}. {} - Price: ${} | 24h Change: {}% | Volume: {}",
                    i + 1,
                    product.product_id,
                    price,
                    change,
                    volume
                );
            }
        }
        Err(e) => {
            println!("   ✗ Failed to fetch market summary: {}", e);
        }
    }

    let separator = "=".repeat(70);
    println!("\n{}", separator);
    println!("Test completed successfully! ✓");
    println!("{}", separator);

    Ok(())
}

