use nautilus_model::defi::Blockchain;

pub fn check_infura_rpc_provider(chain: &Blockchain) -> Option<String> {
    if let Ok(infura_api_key) = std::env::var("INFURA_API_KEY") {
        return match chain {
            Blockchain::Ethereum => Some(format!("https://mainnet.infura.io/v3/{infura_api_key}")),
            Blockchain::Polygon => Some(format!(
                "https://polygon-mainnet.infura.io/v3/{infura_api_key}"
            )),
            Blockchain::Base => Some(format!(
                "https://base-mainnet.infura.io/v3/{infura_api_key}"
            )),
            Blockchain::Optimism => Some(format!(
                "https://optimism-mainnet.infura.io/v3/{infura_api_key}"
            )),
            Blockchain::Arbitrum => Some(format!(
                "https://arbitrum-mainnet.infura.io/v3/{infura_api_key}"
            )),
            _ => None, // We can specify other chains here
        };
    }

    None
}
