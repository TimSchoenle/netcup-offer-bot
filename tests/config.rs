//! The layered configuration, exercised through the names an operator actually sets.
//!
//! `terrace-config` owns the layering and tests it; what these pin is that this crate wires it
//! to the right variables, and that the blocks deserialise with the defaults documented in
//! `README.md` — which is why the expected values below are literals rather than references to
//! the constants that produce them.
//!
//! Every name is derived from [`config::terrace`] rather than spelled out, because the harness
//! is built over the loader the process itself boots through. A test that wrote
//! `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE` by hand would keep passing after the loader
//! stopped reading it, while testing a variable nothing sets.
//!
//! These live in a test binary of their own rather than beside the code, because the harness
//! manipulates the *process* environment: it starts each jail with an empty one, which unsets
//! `TMP` along with everything else, and a `tempfile` call on another test thread then has
//! nowhere to write. A separate binary is a separate process, so the isolation is real rather
//! than a convention future tests have to remember.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use netcup_offer_bot::config::{self, Config};
use secrecy::ExposeSecret;
use terrace_config::testing::Harness;
use tracing::Level;

const WEB_HOOK: &str = "https://discord.com/api/webhooks/";

/// The keys, as the loader spells them. Every layer derives its own spelling from these, so a
/// test names a key once and the harness decides whether that means a variable or a file.
const WEBHOOK_KEY: &str = "discord.webhook_url";
const CHECK_INTERVAL_KEY: &str = "feed.check_interval_secs";

/// A sandbox over the loader this crate actually boots through.
fn harness() -> Harness {
    Harness::over(config::terrace())
}

/// The required keys and nothing else: what an operator has to set, plus every default that
/// fills in around it.
#[test]
fn env_supplies_required_keys_and_defaults_fill_the_rest() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);

        let config: Config = jail.load()?;
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
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("metrics.ip", "0.0.0.0");
        jail.env_key("metrics.port", 9999);
        jail.env_key("telemetry.log_level", "debug");
        jail.env_key("telemetry.sentry_dsn", "https://sentry/1");

        let config: Config = jail.load()?;
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
///
/// Written with `jail.write` rather than `jail.config`, which is the one place in this file
/// where deriving the name would test the wrong thing: `jail.config` sets
/// `$NETCUP_OFFER_BOT_CONFIG` as well, and the default path is exactly what is under test.
#[test]
#[allow(
    clippy::duration_suboptimal_units,
    reason = "the literal mirrors check_interval_secs in the TOML above it"
)]
fn a_toml_file_supplies_the_whole_configuration() {
    harness().run(|jail| {
        jail.write(
            "config.toml",
            "[discord]\n\
             webhook_url = \"https://discord.com/api/webhooks/toml\"\n\
             [feed]\n\
             check_interval_secs = 180\n\
             [metrics]\n\
             port = 1234\n",
        )?;

        let config: Config = jail.load()?;
        assert_eq!(
            config.discord.webhook_url.expose_secret(),
            "https://discord.com/api/webhooks/toml"
        );
        assert_eq!(config.feed.check_interval(), Duration::from_secs(180));
        assert_eq!(config.metrics.port, 1234);
        // An untouched sibling of an overridden key still defaults.
        assert_eq!(config.metrics.ip, "127.0.0.1".parse::<IpAddr>().unwrap());
        Ok(())
    });
}

/// The shape a Kubernetes `Secret` mounted as a volume has: one file per key. A placeholder in
/// a `ConfigMap`'s TOML cannot win over the real webhook.
#[test]
fn a_secrets_directory_outranks_the_toml_layer() {
    harness().run(|jail| {
        jail.config(
            "[discord]\n\
             webhook_url = \"https://discord.com/api/webhooks/placeholder\"\n\
             [feed]\n\
             check_interval_secs = 180\n",
        )?;
        jail.secret_key(WEBHOOK_KEY, WEB_HOOK)?;

        let config: Config = jail.load()?;
        assert_eq!(config.discord.webhook_url.expose_secret(), WEB_HOOK);
        Ok(())
    });
}

/// The mount as the kubelet actually writes it, rather than as a directory of plain files.
///
/// The keys are symlinks into `..data`, which is itself a symlink to a timestamped generation
/// directory. `DirEntry::metadata()` does not follow symlinks, so a provider that asks it
/// whether an entry is a file reports every real key as "not a file" and the service boots on
/// compiled defaults with no error anywhere — silently posting nothing, in this deployment's
/// case. Only this layout reproduces that, which is why it is a test and not a variant of the
/// one above.
#[test]
#[cfg(unix)]
fn a_projected_secret_volume_supplies_the_webhook() {
    harness().run(|jail| {
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.secrets_volume()
            .file("discord__webhook_url", WEB_HOOK)
            .symlinked()
            .create()?;

        let config: Config = jail.load()?;
        assert_eq!(config.discord.webhook_url.expose_secret(), WEB_HOOK);
        Ok(())
    });
}

/// Docker's `_FILE` convention, for a deployment that mounts one secret rather than a
/// directory of them.
#[test]
fn file_indirection_supplies_a_single_key() {
    harness().run(|jail| {
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.indirection(WEBHOOK_KEY, WEB_HOOK)?;

        let config: Config = jail.load()?;
        assert_eq!(config.discord.webhook_url.expose_secret(), WEB_HOOK);
        Ok(())
    });
}

/// A key supplied both by the environment and by a mounted file fails the boot instead of
/// resolving by precedence: an environment variable left behind by a half-finished migration
/// would otherwise keep the process posting to a webhook that has since been rotated.
#[test]
fn a_key_supplied_twice_is_refused() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.secret_key(WEBHOOK_KEY, "https://rotated/hook")?;

        let error = jail
            .load::<Config>()
            .expect_err("two sources define discord.webhook_url");
        assert!(
            error.to_string().contains(WEBHOOK_KEY),
            "the error must name the key: {error}"
        );
        Ok(())
    });
}

/// The report an operator reads when the mount is in place and the old webhook is still being
/// posted to.
///
/// It names the layer, which is the half a value assertion cannot make: a `secret_key` that a
/// forgotten `env_key` is shadowing loads perfectly well and pins nothing.
#[test]
fn the_explanation_names_the_layer_that_supplied_the_webhook() {
    harness().run(|jail| {
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.secret_key(WEBHOOK_KEY, WEB_HOOK)?;

        let explanation = jail.explain()?;
        let origin = explanation
            .origin(WEBHOOK_KEY)
            .expect("the mounted key is reported");
        assert!(
            matches!(
                origin.effective(),
                terrace_config::explain::Layer::SecretsFile(_)
            ),
            "the mount has to be the effective layer: {origin:?}"
        );
        assert!(
            origin.shadowed().is_empty(),
            "nothing shadows it: {origin:?}"
        );
        Ok(())
    });
}

#[test]
fn a_missing_required_key_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(CHECK_INTERVAL_KEY, 42);

        assert!(jail.load::<Config>().is_err());
        Ok(())
    });
}

#[test]
fn an_unparsable_check_interval_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, "soon");

        assert!(jail.load::<Config>().is_err());
        Ok(())
    });
}

#[test]
fn an_unparsable_metric_ip_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("metrics.ip", "abcde");

        assert!(jail.load::<Config>().is_err());
        Ok(())
    });
}

#[test]
fn an_unparsable_metric_port_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("metrics.port", "abcde");

        assert!(jail.load::<Config>().is_err());
        Ok(())
    });
}

/// `FATAL` and `ALL` were documented by the previous system and neither is a `tracing` level,
/// so the error has to name the value and the accepted set rather than read as "invalid level".
#[test]
fn an_unknown_log_level_names_the_accepted_set() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("telemetry.log_level", "FATAL");

        let error = jail
            .load::<Config>()
            .expect_err("FATAL is not a tracing level");
        let message = error.to_string();
        assert!(message.contains("FATAL"), "must name the value: {message}");
        assert!(message.contains("TRACE"), "must name the set: {message}");
        Ok(())
    });
}
