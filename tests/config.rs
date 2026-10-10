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

use netcup_offer_bot::config::{self, Config, LogLevel, SentryLevel};
use secrecy::ExposeSecret;
use terrace_config::testing::Harness;

const WEB_HOOK: &str = "https://discord.com/api/webhooks/";
/// Spelled the way Sentry spells one, so a test that stops asserting the value still fails if the
/// key stops being read as a DSN.
const DSN: &str = "https://key@sentry.example/42";

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
        assert_eq!(config.telemetry.log_level, LogLevel::Info);
        // A deployment that says nothing about Sentry gets no client and no egress. The block is
        // `#[serde(default)]` twice over — once on the field, once on the struct — so a missing
        // section has to materialise rather than fail the boot of a deployment that has never
        // heard of it.
        assert!(!config.telemetry.sentry.enabled);
        assert!(config.telemetry.sentry.dsn.is_none());
        assert_eq!(config.telemetry.sentry.capture_level, SentryLevel::Error);
        assert_eq!(config.telemetry.sentry.breadcrumb_level, SentryLevel::Info);
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
        jail.env_key("telemetry.sentry.enabled", true);
        jail.env_key("telemetry.sentry.dsn", DSN);
        jail.env_key("telemetry.sentry.traces_sample_rate", "0.25");
        jail.env_key("telemetry.sentry.capture_level", "warn");

        let config: Config = jail.load()?;
        assert_eq!(
            config.metrics.socket(),
            "0.0.0.0:9999".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(config.telemetry.log_level, LogLevel::Debug);

        let sentry = &config.telemetry.sentry;
        assert!(sentry.enabled);
        assert_eq!(sentry.dsn.as_ref().unwrap().expose_secret(), DSN);
        assert!((sentry.traces_sample_rate - 0.25).abs() < f32::EPSILON);
        assert_eq!(sentry.capture_level, SentryLevel::Warn);
        Ok(())
    });
}

/// The DSN as the chart supplies it: a mounted `Secret`, one file per key.
///
/// The Sentry keys are two levels deep, which is one level deeper than every other block here,
/// so this is the assertion that the `__` spelling reaches all the way down — and that a
/// credential can arrive as a file rather than as a variable `docker inspect` prints.
#[test]
fn a_mounted_secret_supplies_the_sentry_dsn() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("telemetry.sentry.enabled", true);
        jail.secret_key("telemetry.sentry.dsn", DSN)?;

        let config: Config = jail.load()?;
        assert!(config.telemetry.sentry.enabled);
        assert_eq!(
            config
                .telemetry
                .sentry
                .dsn
                .as_ref()
                .unwrap()
                .expose_secret(),
            DSN
        );
        Ok(())
    });
}

/// The key `telemetry.sentry.dsn` replaced, refused rather than ignored.
///
/// Silently ignoring it is what would take a deployment's error reporting away in the upgrade
/// that renamed the key: the variable stops being read, `telemetry.sentry.enabled` defaults to
/// `false`, and nothing anywhere says so. The message has to name the replacement, because the
/// operator reading it is holding the old spelling.
#[test]
fn the_removed_sentry_dsn_key_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("telemetry.sentry_dsn", DSN);

        let error = jail
            .load::<Config>()
            .expect_err("`telemetry.sentry_dsn` no longer exists");
        let message = error.to_string();
        assert!(
            message.contains("telemetry.sentry.dsn"),
            "must name the replacement: {message}"
        );
        assert!(
            message.contains("telemetry.sentry.enabled"),
            "must say that a DSN alone no longer switches Sentry on: {message}"
        );
        Ok(())
    });
}

/// A key nobody declared fails the load rather than being dropped.
///
/// The quiet failure this replaces: `smaple_rate` in a chart's `ConfigMap` is a volume cap that
/// was never applied, and the boot said nothing about it. The contract already published
/// `additionalProperties: false` at every level, so this is the loader catching up with the
/// document rather than a new claim about the configuration surface.
#[test]
fn a_misspelt_key_fails_the_load() {
    harness().run(|jail| {
        jail.config(
            "[discord]\n\
             webhook_url = \"https://discord.com/api/webhooks/toml\"\n\
             [feed]\n\
             check_interval_secs = 180\n\
             [telemetry.sentry]\n\
             smaple_rate = 0.5\n",
        )?;

        let error = jail
            .load::<Config>()
            .expect_err("`smaple_rate` is not a key of telemetry.sentry");
        let message = error.to_string();
        assert!(
            message.contains("smaple_rate"),
            "must name the key it refused: {message}"
        );
        Ok(())
    });
}

/// A prefixed variable that spells no key fails the load too: the root is closed like every
/// block under it.
///
/// `NETCUP_OFFER_BOT_LOG_LEVEL` is what a deployment reaches for when it means
/// `NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL`, and until now it was read by nothing and reported by
/// nobody. The loader's own variables are not in this set — `$NETCUP_OFFER_BOT_CONFIG` and
/// `$NETCUP_OFFER_BOT_SECRETS_DIR` are filtered out of the environment layer before it is
/// merged, which is what makes closing the root safe and is covered by the TOML tests below.
#[test]
fn a_stray_prefixed_variable_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("log_level", "debug");

        let error = jail
            .load::<Config>()
            .expect_err("`log_level` is not a key of the root");
        let message = error.to_string();
        assert!(
            message.contains("log_level"),
            "must name the variable it refused: {message}"
        );
        Ok(())
    });
}

/// A rate the loader cannot parse fails the boot rather than falling back to the default, which
/// would be a deployment that thinks it is tracing and is not.
#[test]
fn an_unparsable_traces_sample_rate_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("telemetry.sentry.traces_sample_rate", "a fifth");

        assert!(jail.load::<Config>().is_err());
        Ok(())
    });
}

/// `off`, `error`, `warn`, `info`, `debug`, `trace` — and nothing else. The error has to name the
/// set, for the reason the log level's does.
#[test]
fn an_unknown_capture_level_fails_the_load() {
    harness().run(|jail| {
        jail.env_key(WEBHOOK_KEY, WEB_HOOK);
        jail.env_key(CHECK_INTERVAL_KEY, 42);
        jail.env_key("telemetry.sentry.capture_level", "fatal");

        assert!(jail.load::<Config>().is_err());
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

/// A webhook over anything but TLS fails the boot, and the refusal does not repeat the value: the
/// URL is the credential, and the error is printed to stdout and reported to Sentry.
#[test]
fn a_webhook_that_is_not_https_is_refused() {
    const TOKEN: &str = "plaintext-token";
    for webhook in [
        "http://discord.com/api/webhooks/1/plaintext-token",
        "discord.com/api/webhooks/1/plaintext-token",
    ] {
        harness().run(|jail| {
            jail.env_key(WEBHOOK_KEY, webhook);
            jail.env_key(CHECK_INTERVAL_KEY, 42);

            let error = jail
                .load::<Config>()
                .expect_err("only an `https` webhook loads");
            let rendered = format!("{error} {error:?}");
            assert!(
                rendered.contains("Discord webhook"),
                "the error must say what was refused: {rendered}"
            );
            assert!(
                !rendered.contains(TOKEN),
                "the error must not carry the webhook: {rendered}"
            );
            Ok(())
        });
    }
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

/// Every spelling the key accepts, and the level each one means.
///
/// The accepted set is the variant list of [`LogLevel`] and nothing else, so this is the whole
/// of it: five lower-case names, one per level.
#[test]
fn every_log_level_spelling_loads() {
    for (spelling, expected) in [
        ("trace", LogLevel::Trace),
        ("debug", LogLevel::Debug),
        ("info", LogLevel::Info),
        ("warn", LogLevel::Warn),
        ("error", LogLevel::Error),
    ] {
        harness().run(|jail| {
            jail.env_key(WEBHOOK_KEY, WEB_HOOK);
            jail.env_key(CHECK_INTERVAL_KEY, 42);
            jail.env_key("telemetry.log_level", spelling);

            let config: Config = jail.load()?;
            assert_eq!(config.telemetry.log_level, expected);
            Ok(())
        });
    }
}

/// The spellings that stopped working, refused rather than folded.
///
/// `INFO` and `3` both loaded while the key was parsed by `Level::from_str`, which folds case
/// and takes `1`–`5` as well. Neither is a spelling any document ever published, and keeping
/// them would mean maintaining a list of aliases beside the variants — so a deployment carrying
/// one is stopped at boot and told, rather than moved to a level nobody wrote down.
#[test]
fn upper_case_and_numeric_log_levels_are_refused() {
    for refused in ["INFO", "Info", "3"] {
        harness().run(|jail| {
            jail.env_key(WEBHOOK_KEY, WEB_HOOK);
            jail.env_key(CHECK_INTERVAL_KEY, 42);
            jail.env_key("telemetry.log_level", refused);

            let error = jail
                .load::<Config>()
                .expect_err("only the lower-case spellings load");
            let message = error.to_string();
            // Case-folded, because the layer that supplied the value is what decides how the
            // key is spelled back: the environment layer names it `TELEMETRY.LOG_LEVEL`.
            assert!(
                message.to_ascii_lowercase().contains("telemetry.log_level"),
                "must name the key it refused: {message}"
            );
            assert!(
                message.contains(refused),
                "must name the value it refused: {message}"
            );
            Ok(())
        });
    }
}

/// The documented set and the accepted set, checked against each other.
///
/// One derive publishes the variants and another deserialises them, so the two agree by
/// construction — but only for as long as nothing is added beside them. A `deserialize_with`,
/// a `FromStr` or a literal spelling list would each split the two apart again, and this is
/// what notices. Compiled with the generator, which is where a published set exists at all.
#[test]
#[cfg(feature = "config-schema")]
fn every_published_log_level_spelling_loads() {
    let schema = config::schema().expect("the schema should describe");
    let key = schema
        .keys
        .iter()
        .find(|key| key.path == "telemetry.log_level")
        .expect("`telemetry.log_level` should be described");

    assert!(
        !key.values.is_empty(),
        "the key publishes no values: {key:?}"
    );

    for value in &key.values {
        harness().run(|jail| {
            jail.env_key(WEBHOOK_KEY, WEB_HOOK);
            jail.env_key(CHECK_INTERVAL_KEY, 42);
            jail.env_key("telemetry.log_level", value);

            jail.load::<Config>()?;
            Ok(())
        });
    }
}

/// A value that is no level at all. `FATAL` and `ALL` were documented by a previous system and
/// neither is one, so the error has to name the key an operator has to go and fix.
#[test]
fn an_unknown_log_level_names_the_key() {
    for unknown in ["chatty", "FATAL"] {
        harness().run(|jail| {
            jail.env_key(WEBHOOK_KEY, WEB_HOOK);
            jail.env_key(CHECK_INTERVAL_KEY, 42);
            jail.env_key("telemetry.log_level", unknown);

            let error = jail.load::<Config>().expect_err("neither value is a level");
            let message = error.to_string();
            assert!(
                message.to_ascii_lowercase().contains("telemetry.log_level"),
                "must name the key it refused: {message}"
            );
            assert!(
                message.contains(unknown),
                "must name the value it refused: {message}"
            );
            assert!(
                message.contains("`trace`"),
                "must name the set it accepts: {message}"
            );
            Ok(())
        });
    }
}
