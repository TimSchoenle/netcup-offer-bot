//! The layered configuration, exercised through the names an operator actually sets.
//!
//! `terrace-config` owns the layering and tests it; what these pin is that this crate wires it
//! to the right variables, and that the blocks deserialise with the defaults documented in
//! `README.md` — which is why the expected values below are literals rather than references to
//! the constants that produce them.
//!
//! These live in a test binary of their own rather than beside the code, because
//! [`figment::Jail`] manipulates the *process* environment: `clear_env` unsets `TMP` along with
//! everything else, and a `tempfile` call on another test thread then has nowhere to write. A
//! separate binary is a separate process, so the isolation is real rather than a convention
//! future tests have to remember.

// Every test body is a closure passed to `Jail::expect_with`, which fixes the error type to the
// large `figment::Error`. Nothing here can box it, and no `Err` is ever constructed on a hot
// path.
#![expect(
    clippy::result_large_err,
    reason = "figment::Jail::expect_with fixes the closure's error type"
)]

use std::net::SocketAddr;
use std::time::Duration;

use netcup_offer_bot::config::Config;
use secrecy::ExposeSecret;
use tracing::Level;

const WEB_HOOK: &str = "https://discord.com/api/webhooks/";
const ENV_WEB_HOOK: &str = "NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL";
const ENV_CHECK_INTERVAL: &str = "NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS";
const ENV_SECRETS_DIR: &str = "NETCUP_OFFER_BOT_SECRETS_DIR";

/// The required keys and nothing else: what an operator has to set, plus every default that
/// fills in around it.
#[test]
fn env_supplies_required_keys_and_defaults_fill_the_rest() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "42");

        let config = Config::load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(config.discord.webhook_url.expose_secret(), WEB_HOOK);
        assert_eq!(config.feed.check_interval(), Duration::from_secs(42));
        assert_eq!(
            config.metrics.socket(),
            "127.0.0.1:9184".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(config.telemetry.log_level, Level::INFO);
        assert!(config.telemetry.sentry_dsn.is_none());
        Ok(())
    });
}

/// Every optional block, set through the environment.
#[test]
fn env_overrides_every_default() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "42");
        jail.set_env("NETCUP_OFFER_BOT_METRICS__IP", "0.0.0.0");
        jail.set_env("NETCUP_OFFER_BOT_METRICS__PORT", "9999");
        jail.set_env("NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL", "debug");
        jail.set_env("NETCUP_OFFER_BOT_TELEMETRY__SENTRY_DSN", "https://sentry/1");

        let config = Config::load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(
            config.metrics.socket(),
            "0.0.0.0:9999".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(config.telemetry.log_level, Level::DEBUG);
        assert_eq!(
            config.telemetry.sentry_dsn.unwrap().expose_secret(),
            "https://sentry/1"
        );
        Ok(())
    });
}

/// The TOML layer alone is enough to boot, and `./config.toml` is found with nothing pointing
/// at it.
#[test]
fn a_toml_file_supplies_the_whole_configuration() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.create_file(
            "config.toml",
            "[discord]\n\
             webhook_url = \"https://discord.com/api/webhooks/toml\"\n\
             [feed]\n\
             check_interval_secs = 180\n\
             [metrics]\n\
             port = 1234\n",
        )?;

        let config = Config::load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(
            config.discord.webhook_url.expose_secret(),
            "https://discord.com/api/webhooks/toml"
        );
        assert_eq!(config.feed.check_interval(), Duration::from_secs(180));
        assert_eq!(config.metrics.port, 1234);
        // An untouched sibling of an overridden key still defaults.
        assert_eq!(
            config.metrics.ip,
            "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
        );
        Ok(())
    });
}

/// The shape a Kubernetes `Secret` mounted as a volume has: one file per key. A placeholder in
/// a `ConfigMap`'s TOML cannot win over the real webhook.
#[test]
fn a_secrets_directory_outranks_the_toml_layer() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.create_file(
            "config.toml",
            "[discord]\n\
             webhook_url = \"https://discord.com/api/webhooks/placeholder\"\n\
             [feed]\n\
             check_interval_secs = 180\n",
        )?;
        jail.create_dir("secrets")?;
        jail.create_file("secrets/discord__webhook_url", WEB_HOOK)?;
        jail.set_env(ENV_SECRETS_DIR, jail.directory().join("secrets").display());

        let config = Config::load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(config.discord.webhook_url.expose_secret(), WEB_HOOK);
        Ok(())
    });
}

/// Docker's `_FILE` convention, for a deployment that mounts one secret rather than a
/// directory of them.
#[test]
fn file_indirection_supplies_a_single_key() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_CHECK_INTERVAL, "42");
        jail.create_file("webhook", WEB_HOOK)?;
        jail.set_env(
            "NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE",
            jail.directory().join("webhook").display(),
        );

        let config = Config::load().map_err(|e| e.to_string()).unwrap();
        assert_eq!(config.discord.webhook_url.expose_secret(), WEB_HOOK);
        Ok(())
    });
}

/// A key supplied both by the environment and by a mounted file fails the boot instead of
/// resolving by precedence: an environment variable left behind by a half-finished migration
/// would otherwise keep the process posting to a webhook that has since been rotated.
#[test]
fn a_key_supplied_twice_is_refused() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "42");
        jail.create_dir("secrets")?;
        jail.create_file("secrets/discord__webhook_url", "https://rotated/hook")?;
        jail.set_env(ENV_SECRETS_DIR, jail.directory().join("secrets").display());

        let error = Config::load().expect_err("two sources define discord.webhook_url");
        assert!(
            error.to_string().contains("discord.webhook_url"),
            "the error must name the key: {error}"
        );
        Ok(())
    });
}

#[test]
fn a_missing_required_key_fails_the_load() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_CHECK_INTERVAL, "42");

        assert!(Config::load().is_err());
        Ok(())
    });
}

#[test]
fn an_unparsable_check_interval_fails_the_load() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "soon");

        assert!(Config::load().is_err());
        Ok(())
    });
}

#[test]
fn an_unparsable_metric_ip_fails_the_load() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "42");
        jail.set_env("NETCUP_OFFER_BOT_METRICS__IP", "abcde");

        assert!(Config::load().is_err());
        Ok(())
    });
}

#[test]
fn an_unparsable_metric_port_fails_the_load() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "42");
        jail.set_env("NETCUP_OFFER_BOT_METRICS__PORT", "abcde");

        assert!(Config::load().is_err());
        Ok(())
    });
}

/// `FATAL` and `ALL` were documented by the previous system and neither is a `tracing` level,
/// so the error has to name the value and the accepted set rather than read as "invalid level".
#[test]
fn an_unknown_log_level_names_the_accepted_set() {
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.set_env(ENV_WEB_HOOK, WEB_HOOK);
        jail.set_env(ENV_CHECK_INTERVAL, "42");
        jail.set_env("NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL", "FATAL");

        let error = Config::load().expect_err("FATAL is not a tracing level");
        let message = error.to_string();
        assert!(message.contains("FATAL"), "must name the value: {message}");
        assert!(message.contains("TRACE"), "must name the set: {message}");
        Ok(())
    });
}
