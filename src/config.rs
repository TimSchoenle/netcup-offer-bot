use crate::error::Error;
use secrecy::SecretString;
use std::net::SocketAddr;
use std::time::Duration;

const DEFAULT_METRIC_IP: &str = "127.0.0.1";
const DEFAULT_METRIC_PORT: u16 = 9184;

#[derive(Debug, serde::Deserialize)]
struct RawConfig {
    web_hook: SecretString,
    check_interval: u64,
    metric_ip: Option<String>,
    metric_port: Option<u16>,
}

#[derive(Debug)]
pub struct Config {
    pub discord_webhook_url: SecretString,
    pub check_interval: Duration,
    pub metric_socket: SocketAddr,
}

impl TryFrom<RawConfig> for Config {
    type Error = crate::Error;

    fn try_from(value: RawConfig) -> Result<Self, Self::Error> {
        let check_interval = Duration::from_secs(value.check_interval);
        let metric_ip = value
            .metric_ip
            .unwrap_or_else(|| DEFAULT_METRIC_IP.to_string());
        let metric_port = value.metric_port.unwrap_or(DEFAULT_METRIC_PORT);
        let metric_ip = match metric_ip.parse::<std::net::IpAddr>() {
            Ok(ip) => ip,
            Err(_) => {
                return Err(Error::custom(format!(
                    "Invalid metric ip address: {metric_ip}"
                )));
            }
        };

        let metric_socket = SocketAddr::new(metric_ip, metric_port);
        Ok(Self {
            discord_webhook_url: value.web_hook,
            check_interval,
            metric_socket,
        })
    }
}

impl Config {
    pub fn get_configurations() -> crate::Result<Self> {
        config::Config::builder()
            .add_source(config::Environment::default())
            .build()
            .map_err(|e| Error::custom(format!("Can't parse config: {e}")))?
            .try_deserialize::<RawConfig>()
            .map_err(|e| Error::custom(format!("Failed to deserialize configuration: {e}")))?
            .try_into()
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    const ENV_WEB_HOOK: &str = "WEB_HOOK";
    const ENV_CHECK_INTERVAL: &str = "CHECK_INTERVAL";
    const ENV_METRIC_IP: &str = "METRIC_IP";
    const ENV_METRIC_PORT: &str = "METRIC_PORT";

    const CORRECT_WEB_HOOK: &str = "https://discord.com/api/webhooks/";
    const CORRECT_CHECK_INTERVAL: &str = "42";
    const CORRECT_METRIC_IP: &str = "127.0.0.1";
    const CORRECT_METRIC_PORT: &str = "9184";

    #[test]
    fn test_from_env_missing_env() {
        temp_env::with_vars_unset(vec![ENV_WEB_HOOK, ENV_CHECK_INTERVAL], || {
            let result = Config::get_configurations();
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_from_env_minimal() {
        temp_env::with_vars(
            vec![
                (ENV_WEB_HOOK, Some(CORRECT_WEB_HOOK)),
                (ENV_CHECK_INTERVAL, Some(CORRECT_CHECK_INTERVAL)),
            ],
            || {
                let result = Config::get_configurations();
                assert!(result.is_ok());

                let config = result.unwrap();
                assert_eq!(config.discord_webhook_url.expose_secret(), CORRECT_WEB_HOOK);
                assert_eq!(
                    config.check_interval,
                    Duration::from_secs(CORRECT_CHECK_INTERVAL.parse().unwrap())
                );
            },
        );
    }

    #[test]
    fn test_from_env_full() {
        temp_env::with_vars(
            vec![
                (ENV_WEB_HOOK, Some(CORRECT_WEB_HOOK)),
                (ENV_CHECK_INTERVAL, Some(CORRECT_CHECK_INTERVAL)),
                (ENV_METRIC_IP, Some(CORRECT_METRIC_IP)),
                (ENV_METRIC_PORT, Some(CORRECT_METRIC_PORT)),
            ],
            || {
                let result = Config::get_configurations();
                assert!(result.is_ok());

                let config = result.unwrap();
                assert_eq!(config.discord_webhook_url.expose_secret(), CORRECT_WEB_HOOK);
                assert_eq!(
                    config.check_interval,
                    Duration::from_secs(CORRECT_CHECK_INTERVAL.parse().unwrap())
                );
                assert_eq!(
                    config.metric_socket,
                    SocketAddr::new(
                        CORRECT_METRIC_IP.parse().unwrap(),
                        CORRECT_METRIC_PORT.parse().unwrap(),
                    )
                );
            },
        );
    }

    #[test]
    fn test_from_env_invalid_check_interval() {
        temp_env::with_vars(
            vec![
                (ENV_WEB_HOOK, Some(CORRECT_WEB_HOOK)),
                (ENV_CHECK_INTERVAL, Some("d")),
            ],
            || {
                let result = Config::get_configurations();
                assert!(result.is_err());
            },
        );
    }

    #[test]
    fn test_from_env_invalid_metric_ip() {
        temp_env::with_vars(
            vec![
                (ENV_WEB_HOOK, Some(CORRECT_WEB_HOOK)),
                (ENV_CHECK_INTERVAL, Some(CORRECT_CHECK_INTERVAL)),
                (ENV_METRIC_IP, Some("abcde")),
                (ENV_METRIC_PORT, Some(CORRECT_METRIC_PORT)),
            ],
            || {
                let result = Config::get_configurations();
                assert!(result.is_err());
            },
        );
    }

    #[test]
    fn test_from_env_invalid_metric_port() {
        temp_env::with_vars(
            vec![
                (ENV_WEB_HOOK, Some(CORRECT_WEB_HOOK)),
                (ENV_CHECK_INTERVAL, Some(CORRECT_CHECK_INTERVAL)),
                (ENV_METRIC_IP, Some(CORRECT_METRIC_IP)),
                (ENV_METRIC_PORT, Some("abcde")),
            ],
            || {
                let result = Config::get_configurations();
                assert!(result.is_err());
            },
        );
    }

    #[test]
    fn test_from_env_invalid_unicode_character() {
        temp_env::with_vars(
            vec![(ENV_WEB_HOOK, Some("⛷")), (ENV_CHECK_INTERVAL, Some("⛷"))],
            || {
                let result = Config::get_configurations();
                assert!(result.is_err());
            },
        );
    }
}
