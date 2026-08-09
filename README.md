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
![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/TimSchoenle/netcup-offer-bot/build.yml)
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

```shell
  docker run \
    --name netcup-offer-bot \
    -e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL="https://discord.com/api/webhooks/..." \
    -e NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS="180" \
    -v netcup-offer-bot-data:/app/data \
    -d \
    timmi6790/netcup-offer-bot:latest
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

**Nesting is `__` (two underscores)** — a single underscore is part of a field name. Case is
folded, so `discord.webhook_url` is `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` as a variable and
`discord__webhook_url` as a file name.

#### Keys

| Key                                        | Required | Default     | Description                                              |
|--------------------------------------------|----------|-------------|----------------------------------------------------------|
| `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL`    | X        |             | Discord webhook the offers are posted to                 |
| `NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS` | X      |             | Seconds between two RSS feed checks                      |
| `NETCUP_OFFER_BOT_METRICS__IP`             |          | `127.0.0.1` | Prometheus exporter address                              |
| `NETCUP_OFFER_BOT_METRICS__PORT`           |          | `9184`      | Prometheus exporter port                                 |
| `NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL`    |          | `INFO`      | One of `TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`         |
| `NETCUP_OFFER_BOT_TELEMETRY__SENTRY_DSN`   |          |             | Sentry DSN; unset disables Sentry entirely               |

Two further variables are read to decide what the layers *are*, and so cannot themselves be
supplied by a layer:

| Key                             | Default       | Description                                                        |
|---------------------------------|---------------|--------------------------------------------------------------------|
| `NETCUP_OFFER_BOT_CONFIG`       | `config.toml` | The TOML layer: a file, or a directory whose `*.toml` are merged   |
| `NETCUP_OFFER_BOT_SECRETS_DIR`  |               | Directory of key-named files. Unset disables the layer; set but unreadable fails the boot |

#### `config.toml`

```toml
[discord]
webhook_url = "https://discord.com/api/webhooks/..."

[feed]
check_interval_secs = 180

[metrics]
ip = "0.0.0.0"
port = 9184

[telemetry]
log_level = "INFO"
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
    timmi6790/netcup-offer-bot:latest
  ```

Or Docker's `_FILE` convention, for a single secret:

```shell
  -e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE="/run/secrets/webhook"
  ```

## License

Distributed under the MIT License. See [LICENSE](https://github.com/TimSchoenle/netcup-offer-bot/blob/main/LICENSE.md)
for
more information.