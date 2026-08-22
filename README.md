<!--
Generated from .github/templates/README.md.hbs — edit that file, not this one. CI renders it on
every pull request and commits the result back to the branch; a push to master whose README.md
does not match its template fails the `readme` check in .github/workflows/docs.yaml.

Variables come from `cargo run --features config-schema --example readme-variables`:

    version              the [package] version, e.g. 2.0.1
    repository           the GitHub repository, as owner/name
    branch               the branch permanent links point at
    image                the Docker Hub repository the image is published to
    prefix               the prefix every configuration variable carries
    nesting_separator    what separates nesting levels in an environment key
    indirection_suffix   what marks a variable holding a path rather than a value
    config_loader        the table of variables the loader reads
    config_keys          the table of configuration keys
    config_toml          a config.toml carrying every key

The last three are derived from the `Config` type itself, by way of terrace-config's `schema`
feature. Adding a key, changing a default or rewriting a field's doc comment updates this page in
the same commit that changes the code — which is the point: a reference table maintained beside
the type is a table that is wrong by the second release.
-->
<br/>
<p align="center">
  <h3 align="center">Netcup Offer Bot</h3>

  <p align="center">
    <a href="https://github.com/TimSchoenle/netcup-offer-bot/issues">Report Bug</a>
    .
    <a href="https://github.com/TimSchoenle/netcup-offer-bot/issues">Request Feature</a>
  </p>
</p>

<div align="center">

![Docker Image Version (latest semver)](https://img.shields.io/docker/v/timmi6790/netcup-offer-bot)
![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/TimSchoenle/netcup-offer-bot/build.yaml)
![Issues](https://img.shields.io/github/issues/TimSchoenle/netcup-offer-bot)
[![codecov](https://codecov.io/gh/TimSchoenle/netcup-offer-bot/branch/master/graph/badge.svg?token=JEK95V1906)](https://codecov.io/gh/TimSchoenle/netcup-offer-bot)
![License](https://img.shields.io/github/license/TimSchoenle/netcup-offer-bot)

</div>

## About The Project

RSS feed listener to discord webhook for https://www.netcup.com/de/deals

### Installation - Helm chart

- [Helm chart](https://github.com/TimSchoenle/helm-charts/tree/main/charts/netcup-offer-bot)

### Installation - Docker

- [Docker Image](https://hub.docker.com/repository/docker/timmi6790/netcup-offer-bot)

The image is published as a multi-platform manifest for `linux/amd64` and `linux/arm64`. Docker pulls
the matching architecture automatically, so the commands below are identical on both.

#### Quick start

The examples pin `2.2.0`, the release this page was generated from; `latest` tracks the newest.

```shell
  docker run \
    --name netcup-offer-bot \
    -e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL="https://discord.com/api/webhooks/..." \
    -e NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS="180" \
    -v netcup-offer-bot-data:/app/data \
    -d \
    timmi6790/netcup-offer-bot:2.2.0
  ```

## Configuration

Configuration is layered by [terrace-config](https://github.com/TimSchoenle/terrace-config), so the
Discord webhook can arrive as a mounted file rather than as an environment variable that shows up
in `docker inspect` and in the environment of every child process.

Lowest precedence first:

1. The defaults compiled into the config structs.
2. TOML at `$NETCUP_OFFER_BOT_CONFIG`, defaulting to `./config.toml` — a file, or every `*.toml`
   directly inside it when it names a directory, merged in file-name order. A missing file is not
   an error.
3. `NETCUP_OFFER_BOT_`-prefixed environment variables.
4. Every key-named file in `$NETCUP_OFFER_BOT_SECRETS_DIR`.
5. `NETCUP_OFFER_BOT_<KEY>_FILE=/path`, which reads `<KEY>` from that path.

**Layers 3, 4 and 5 are mutually exclusive per key.** A key supplied by two of them fails the boot
naming the key and both sources, rather than being resolved by precedence: a stale environment
variable shadowing a webhook that has since been rotated would otherwise keep the bot posting to
the old one, and the discrepancy would surface long after the deploy that caused it.

**Nesting is `__`** — a single underscore is part of a field name. Case is folded,
so `discord.webhook_url` is `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` as a variable and
`discord__webhook_url` as a file name.

Run with `NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL=DEBUG` and the boot log names the layer
every key was read from, which is what answers "the `Secret` is mounted and the bot is still posting
to the old webhook".

#### The variables read before the layers exist

These decide what the layers *are*, so no layer can supply them.

| Variable | Role | Default | Purpose |
|---|---|---|---|
| `NETCUP_OFFER_BOT_CONFIG` | config | `config.toml` | Names the TOML layer: a file, or a directory whose `*.toml` files are all merged in name order. |
| `NETCUP_OFFER_BOT_SECRETS_DIR` | secrets dir | — | Names a directory of key-named files — a mounted Kubernetes `Secret` volume. Each file supplies the key its name spells. |

#### Keys

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `discord.webhook_url` | `SecretString` | `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` | — | required, secret | Discord webhook the offers are posted to. |
| `feed.check_interval_secs` | `u64` | `NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS` | — | required | Seconds between two RSS feed checks. |
| `metrics.ip` | `IpAddr` | `NETCUP_OFFER_BOT_METRICS__IP` | `127.0.0.1` | — | Address the Prometheus exporter binds. `0.0.0.0` to reach it from outside the container. |
| `metrics.port` | `u16` | `NETCUP_OFFER_BOT_METRICS__PORT` | `9184` | — | Port the Prometheus exporter listens on. |
| `telemetry.log_level` | `Level` | `NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL` | `INFO` | — | The maximum verbosity that reaches stdout: `TRACE`, `DEBUG`, `INFO`, `WARN` or `ERROR`, in any case. |
| `telemetry.sentry_dsn` | `SecretString` | `NETCUP_OFFER_BOT_TELEMETRY__SENTRY_DSN` | unset | secret | Sentry DSN. Unset disables Sentry entirely. |

Every key has two further spellings, both mechanical and both left out of the table to keep it inside
a page: appending `_FILE` to the environment variable names a *file* holding the value,
and the TOML path with `.` replaced by `__` is that key's file name inside the secrets
directory.

#### `config.toml`

Every key, commented out wherever leaving it out changes nothing, so this file and an empty one mean
the same thing to the loader. What is left uncommented is exactly what has to be supplied — and each
of those carries a placeholder rather than a value, so a copy left unedited fails at the key that was
never filled in rather than running on it.

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

# Sentry DSN. Unset disables Sentry entirely.
# Type: SecretString
# Secret: the value below is a placeholder.
# sentry_dsn = "<secret>"
```

#### Secrets from files

A Kubernetes `Secret` mounted as a volume, one file per key — the provider follows the `..data`
indirection a projected volume uses, so the mount works as written:

```shell
  docker run \
    --name netcup-offer-bot \
    -e NETCUP_OFFER_BOT_SECRETS_DIR="/run/secrets" \
    -e NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS="180" \
    -v ./webhook:/run/secrets/discord__webhook_url:ro \
    -v netcup-offer-bot-data:/app/data \
    -d \
    timmi6790/netcup-offer-bot:2.2.0
  ```

Or Docker's `_FILE` convention, for a single secret:

```shell
  -e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE="/run/secrets/webhook"
  ```

#### The config contract

Every release publishes the table above as a machine-readable document, so a chart deploying this
image can be checked against what the image actually reads rather than against a copy of this page
that may have drifted. The committed copy is [`docs/config.contract.json`](docs/config.contract.json).

Each image carries the same document three ways:

| Carrier | What it answers |
|---|---|
| `LABEL dev.terrace.config.*` in the image config blob | Does this image declare a contract, where is its offline copy, and which environment variables are its business — answerable in one registry request, with no layer pull |
| `/config/contract.json` in the image | The offline copy, for a `docker save` tarball or an air-gapped mirror |
| An OCI referrer of type `application/vnd.terrace.config-schema.v1+json` on the pushed digest, cosign-signed | The canonical fetch, tied to the exact build a chart pins |

All three are rendered from the same document by one program:

```shell
cargo run --features config-schema --example config-contract -- --format contract
cargo run --features config-schema --example config-contract -- --format labels
cargo run --features config-schema --example config-contract -- --format dockerfile
```

After changing a configuration key, regenerate the committed copy and the Dockerfile's `LABEL`
region:

```shell
just regenerate
```

It rewrites `docs/config.contract.json` and the region between the `terrace-config:labels` markers
in the `Dockerfile`, so a local run is the fix rather than a report. It writes and never checks —
the checking is `TimSchoenle/actions/actions/rust/config-contract`, which diffs both against the
configuration types on every pull request and then checks the built **image** against the labels
the same generator emitted. The release workflow checks every platform in the index separately,
before anything is attached to the digest or signed.

## License

Distributed under the MIT License. See [LICENSE](https://github.com/TimSchoenle/netcup-offer-bot/blob/master/LICENSE)
for more information.
