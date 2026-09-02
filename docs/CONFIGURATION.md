<!--
Generated from .github/templates/CONFIGURATION.md.hbs. Edit that file, not this one.

Rendered by the same job that renders README.md, from the same payload, so the tables here and
the one there come out of one run of:

    cargo run --quiet --features config-schema --example readme-variables

Nothing in this comment may contain a mustache that is not a real reference.
-->

# Configuration

Every configuration key this service reads, in every spelling that can supply it.

The layering, the two required keys and the key table are in
[the README](../README.md#configuration). What follows is the detail that does not fit on a page
someone reads to decide whether to run this at all.

## The variables read before the layers exist

These decide what the layers are, so no layer can supply them.

| Variable | Role | Default | Purpose |
|---|---|---|---|
| `NETCUP_OFFER_BOT_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `NETCUP_OFFER_BOT_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

## The two further spellings of every key

Both are mechanical, which is why the key table names only the environment variable:

- Appending `_FILE` to the environment variable makes it name a file holding the value.
- The TOML path with `.` replaced by `__` is that key's file name inside
  `$NETCUP_OFFER_BOT_SECRETS_DIR`.

So `discord.webhook_url` is supplied by `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL`, by
`NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE=/path`, or by a file named `discord__webhook_url` in
the secrets directory. Supplying it by two of the three fails the boot.

## `config.toml`

Every key, commented out wherever leaving it out changes nothing, so this file and an empty one
mean the same thing to the loader. What is left uncommented is what has to be supplied, and each
of those carries a placeholder rather than a value: a copy left unedited fails at the key nobody
filled in rather than running on it.

```toml
[discord]
# Discord webhook the offers are posted to.
# Type: SecretString
# Required: nothing loads until this key is supplied.
# Secret: the value below is a placeholder.
webhook_url = "<secret>"

[feed]
# Seconds between two RSS feed checks.
# Type: u64
# Required: nothing loads until this key is supplied.
check_interval_secs = 0

[metrics]
# Address the Prometheus exporter binds. `0.0.0.0` to reach it from outside the container.
# Type: IpAddr
# ip = "127.0.0.1"

# Port the Prometheus exporter listens on.
# Type: u16
# port = 9184

[telemetry]
# The maximum verbosity that reaches stdout: `TRACE`, `DEBUG`, `INFO`, `WARN` or `ERROR`, in any case.
# Type: Level
# log_level = "INFO"

[telemetry.sentry]
# Initialise the Sentry client. `false` installs no client, no panic hook and no layer, so every other key here is inert and nothing leaves the process.
# Type: bool
# enabled = false

# Ingest URL, `https://<key>@<host>/<project>`. Required once `enabled` is set.
# Type: SecretString
# Secret: the value below is a placeholder.
# dsn = "<secret>"

# Environment tag on every event. Defaults to `production`, or `development` for a debug build.
# Type: String
# Unset by default: the value below is only the shape.
# environment = "<value>"

# Release tag on every event. Defaults to the version the binary was built from.
# Type: String
# Unset by default: the value below is only the shape.
# release = "<value>"

# Host tag on every event. Unset, Sentry reports none.
# Type: String
# Unset by default: the value below is only the shape.
# server_name = "<value>"

# Fraction of captured events actually sent, `0.0`–`1.0`.
# Type: f32
# sample_rate = 1.0

# Fraction of traces that are recorded, `0.0`–`1.0`. `0.0` records none.
# Type: f32
# traces_sample_rate = 0.0

# Least severe `tracing` level reported as a Sentry issue: `off`, `error`, `warn`, `info`, `debug` or `trace`.
# Type: SentryLevel — one of: off, error, warn, info, debug, trace
# capture_level = "error"

# Least severe `tracing` level kept as a breadcrumb — the trail attached to the next issue.
# Type: SentryLevel — one of: off, error, warn, info, debug, trace
# breadcrumb_level = "info"

# How many breadcrumbs one event carries.
# Type: usize
# max_breadcrumbs = 100

# Attach a stack trace to events that carry none of their own.
# Type: bool
# attach_stacktraces = true

# How long process exit waits for queued events to drain.
# Type: u64
# shutdown_timeout_secs = 2

# Print the SDK's own diagnostics to stderr. For proving a DSN works, not for running.
# Type: bool
# debug = false
```

## Secrets from files

A Kubernetes `Secret` mounted as a volume, one file per key. The provider follows the `..data`
indirection a projected volume uses, so the mount works as written:

```bash
docker run --rm \
  -e NETCUP_OFFER_BOT_SECRETS_DIR="/run/secrets" \
  -e NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS=900 \
  -v ./webhook:/run/secrets/discord__webhook_url:ro \
  -v netcup-offer-bot-data:/app/data \
  timmi6790/netcup-offer-bot:v3.1.0
```

For a single secret, Docker's own convention reaches the same place:

```bash
-e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE="/run/secrets/webhook"
```

## `telemetry.sentry`

Off unless a deployment switches it on, and the whole block is inert until it does. A DSN is an
egress destination for whatever a log line happens to carry, so turning it on is a decision made
once per deployment rather than one this image makes by shipping a key.

```bash
docker run --rm \
  -e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE="/run/secrets/webhook" \
  -e NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS=900 \
  -e NETCUP_OFFER_BOT_TELEMETRY__SENTRY__ENABLED=true \
  -e NETCUP_OFFER_BOT_TELEMETRY__SENTRY__DSN_FILE="/run/secrets/sentry-dsn" \
  -e NETCUP_OFFER_BOT_TELEMETRY__SENTRY__TRACES_SAMPLE_RATE=0.1 \
  -v netcup-offer-bot-data:/app/data \
  timmi6790/netcup-offer-bot:v3.1.0
```

Four things about it are worth knowing before it is switched on:

- **`ENABLED=true` without a usable DSN fails the boot.** So does a DSN that does not parse, and
  so does a sample rate outside `0.0`–`1.0`. A reporter that reports nowhere is the one failure
  nobody sees: the process boots, serves, and a quiet issue stream looks exactly like a quiet
  week. An empty value counts as no DSN, because an unfilled chart value and a compose
  pass-through both resolve to one.
- **The DSN is a credential** for the project's ingest endpoint, and takes the same three
  spellings the webhook does. Mount it.
- **Tracing is a second switch.** `traces_sample_rate` is `0.0` by default, and at `0.0` no spans
  are built at all — errors are still reported. `0.05`–`0.2` is an ordinary production figure.
- **The Sentry thresholds do not follow `telemetry.log_level`.** `capture_level` decides what
  becomes an issue and `breadcrumb_level` what is kept as the trail attached to the next one, and
  the layer goes on collecting that trail however quiet stdout has been told to be. Setting
  `log_level = "ERROR"` does not empty the breadcrumbs on an issue.

### The key this replaced

`NETCUP_OFFER_BOT_TELEMETRY__SENTRY_DSN` is gone. Supplying it fails the boot
naming `telemetry.sentry.dsn`, rather than being ignored: a rename that resolved silently would
take a deployment's error reporting away in the upgrade that renamed the key, and a DSN alone no
longer switches Sentry on — `telemetry.sentry.enabled` does.

### Builds without Sentry

The client, the panic hook and the `tracing` layer are behind the crate's default `sentry`
feature. `cargo build --release --no-default-features` produces a binary with no Sentry code in
it and no egress path to a third party; the keys above are still read, documented and published
in the contract, so one document describes every build. Such a binary refuses to boot on
`telemetry.sentry.enabled` instead of starting a reporter it has no client for. The published
image is built with default features and reports.

## The config contract

Every release publishes the key table as a machine-readable document, so a chart deploying an
image can be checked against the keys that image reads rather than against a copy of this page.
The committed copy is [config.contract.json](config.contract.json).

Each image carries the same document three ways:

| Carrier | What it answers |
| --- | --- |
| `LABEL dev.terrace.config.*` in the image config blob | Does this image declare a contract, where is its offline copy, and which environment variables are its business. One registry request, no layer pull |
| `/config/contract.json` in the image | The offline copy, for a `docker save` tarball or an air-gapped mirror |
| An OCI referrer of type `application/vnd.terrace.config-schema.v1+json` on the pushed digest, cosign-signed | The canonical fetch, tied to the exact build a chart pins |

One program renders all three:

```bash
cargo run --features config-schema --example config-contract -- --format contract
cargo run --features config-schema --example config-contract -- --format labels
cargo run --features config-schema --example config-contract -- --format dockerfile
```

After changing a configuration key, `just regenerate` rewrites the committed document and the
region between the `terrace-config:labels` markers in the `Dockerfile`. It writes and never
checks. The checking is `TimSchoenle/actions/actions/rust/config-contract`, which diffs both
against the configuration types on every pull request and then checks the built image against
the labels the same generator emitted, one platform at a time, before anything is attached to
the digest or signed.
