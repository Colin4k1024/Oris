use oris_hub::{HubConfig, HubServer};
use std::net::SocketAddr;

const DEFAULT_LOCAL_API_KEY: &str = "dev-local-api-key";
const MAX_SIGNATURE_AGE_SECONDS: i64 = 3600;

fn parse_api_keys(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn validate_config(
    bind_addr: SocketAddr,
    api_keys: &[String],
    signature_max_age_seconds: i64,
) -> anyhow::Result<()> {
    if api_keys.is_empty() {
        anyhow::bail!("HUB_API_KEYS must include at least one non-empty token");
    }

    if !bind_addr.ip().is_loopback() && api_keys.iter().any(|key| key == DEFAULT_LOCAL_API_KEY) {
        anyhow::bail!(
            "HUB_API_KEYS must not contain the default development token when HUB_ADDR binds to a non-loopback address"
        );
    }

    if !(1..=MAX_SIGNATURE_AGE_SECONDS).contains(&signature_max_age_seconds) {
        anyhow::bail!(
            "HUB_SIGNATURE_MAX_AGE_SECONDS must be in the range 1..={MAX_SIGNATURE_AGE_SECONDS}"
        );
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("oris_hub=info".parse().unwrap()),
        )
        .init();

    let bind_addr: SocketAddr = std::env::var("HUB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse()
        .expect("HUB_ADDR must be a valid socket address (e.g. 127.0.0.1:3000)");

    let db_path = std::env::var("HUB_DB_PATH").unwrap_or_else(|_| "hub.db".to_string());
    let subscription_db_path = std::env::var("HUB_SUBSCRIPTION_DB_PATH")
        .unwrap_or_else(|_| "hub-subscriptions.db".to_string());
    let api_keys = parse_api_keys(
        &std::env::var("HUB_API_KEYS").unwrap_or_else(|_| DEFAULT_LOCAL_API_KEY.to_string()),
    );

    let signature_max_age_seconds = std::env::var("HUB_SIGNATURE_MAX_AGE_SECONDS")
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(300);

    validate_config(bind_addr, &api_keys, signature_max_age_seconds)?;

    let config = HubConfig {
        bind_addr,
        db_path,
        subscription_db_path,
        api_keys,
        signature_max_age_seconds,
        ..HubConfig::default()
    };

    println!("Hub listening on {bind_addr}");
    HubServer::new(config).run().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        parse_api_keys, validate_config, DEFAULT_LOCAL_API_KEY, MAX_SIGNATURE_AGE_SECONDS,
    };
    use std::net::SocketAddr;

    #[test]
    fn parse_api_keys_discards_empty_entries() {
        let api_keys = parse_api_keys(" alpha, ,beta ,, gamma ");
        assert_eq!(api_keys, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn validate_config_rejects_default_key_on_non_loopback_bind() {
        let bind_addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
        let api_keys = vec![DEFAULT_LOCAL_API_KEY.to_string(), "extra-key".to_string()];

        let error = validate_config(bind_addr, &api_keys, 300).unwrap_err();

        assert!(error
            .to_string()
            .contains("must not contain the default development token"));
    }

    #[test]
    fn validate_config_rejects_signature_window_above_cap() {
        let bind_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let api_keys = vec![DEFAULT_LOCAL_API_KEY.to_string()];

        let error =
            validate_config(bind_addr, &api_keys, MAX_SIGNATURE_AGE_SECONDS + 1).unwrap_err();

        assert!(error.to_string().contains("HUB_SIGNATURE_MAX_AGE_SECONDS"));
    }
}
