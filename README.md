<!--
Generated from .github/templates/README.md.hbs. Edit that file, not this one.

CI renders it on every pull request and commits the result back to the branch. A push to master
whose README.md does not match its template fails the `README` job in
.github/workflows/docs.yaml.

The configuration half of the payload comes from one command:

    cargo run --quiet --features config-schema --example readme-variables

The rest is derived by TimSchoenle/actions/actions/common/readme-variables, which reads
Cargo.toml and walks docs/. Every string this page quotes that also lives in a manifest arrives
that way, so no release, licence or edition is typed here.

Nothing in this comment may contain a mustache that is not a real reference.
-->

# netcup-offer-bot

Watches the netcup deals RSS feed and posts new offers to a Discord webhook.

[![Release](https://img.shields.io/github/v/release/TimSchoenle/netcup-offer-bot?sort=semver)](https://github.com/TimSchoenle/netcup-offer-bot/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/TimSchoenle/netcup-offer-bot/build.yaml?branch=master)](https://github.com/TimSchoenle/netcup-offer-bot/actions/workflows/build.yaml)
[![Coverage](https://codecov.io/gh/TimSchoenle/netcup-offer-bot/branch/master/graph/badge.svg?token=JEK95V1906)](https://codecov.io/gh/TimSchoenle/netcup-offer-bot)
[![License](https://img.shields.io/github/license/TimSchoenle/netcup-offer-bot)](LICENSE)

## What this is

One process. It polls the netcup deals RSS feed at <https://www.netcup.com/rss/deals/de> and
posts each item it has not seen before to a Discord webhook, as an embed carrying the title,
description, link, publication date and categories.

Seen is decided by publication date. After every round the newest `pubDate` is written to
`./data/feed_state.json`, and an item dated at or before it is skipped. A process that starts
without that file holds no watermark, so it posts everything the feed currently lists.

The configuration table below is generated from the Rust types that load the configuration. So
are [docs/config.contract.json](docs/config.contract.json) and the `dev.terrace.config.*` labels
every image carries, which is what lets a chart be checked against the keys the image reads
rather than against a copy of this page.

## Quick start

```bash
docker run --rm \
  -e NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL="https://discord.com/api/webhooks/..." \
  -e NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS=900 \
  -v netcup-offer-bot-data:/app/data \
  timmi6790/netcup-offer-bot:v2.1.1
```

Those two variables are the only required keys. The volume holds the watermark; drop it and
every restart reposts whatever the feed still lists.

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Usage](#usage)
- [Configuration](#configuration)
- [Operations](#operations)
- [Compatibility](#compatibility)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)

## Features

- **The webhook can arrive as a file.** `NETCUP_OFFER_BOT_<KEY>_FILE` names a path, and a secrets
  directory supplies one key per file, so a mounted Kubernetes `Secret` reaches the process
  without the URL turning up in `docker inspect` or in a child process environment.
- A key supplied by two of the environment, the secrets directory and `_FILE` indirection fails
  the boot naming both sources. Resolving it by precedence would let a stale variable go on
  shadowing a webhook that has since been rotated.
- Delivery retries five times. A `429` waits out the `retry-after` header plus a second; a `5xx`
  or a connection failure backs off two seconds, then four, then eight. The fifth failure gives
  up on that item and counts it.
- A feed payload that does not begin with an `rss` tag is logged as a warning instead of counted
  as a fetch error, because the upstream answers with an HTML page often enough that alerting on
  it would be alerting on netcup's bad minute.
- Four Prometheus metrics, on an exporter that binds `127.0.0.1:9184` by default.
- Errors reach Sentry when a DSN is configured. The image build uploads debug symbols before it
  strips the binary, so a musl release build still symbolicates.

## Installation

### Docker

```bash
docker pull timmi6790/netcup-offer-bot:v2.1.1
```

Published as a multi-platform manifest for `linux/amd64` and `linux/arm64`. Every release is
pushed by digest, signed with cosign, and carries its configuration contract as an OCI referrer
on that digest. Pin by digest in production. The Helm chart does.

### Helm

```bash
helm repo add timschoenle https://timschoenle.github.io/helm-charts
helm install netcup-offer-bot timschoenle/netcup-offer-bot
```

The chart pins the image by digest, and this repository's release workflow bumps it. Its values
are at
[TimSchoenle/helm-charts](https://github.com/TimSchoenle/helm-charts/tree/main/charts/netcup-offer-bot).

### From source

```bash
git clone https://github.com/TimSchoenle/netcup-offer-bot.git
cd netcup-offer-bot
cargo build --release
```

## Usage

The binary takes no arguments. Everything it reads is configuration, and the two required keys
have to come from somewhere before it starts:

```bash
NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL_FILE=./webhook \
NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS=900 \
  cargo run --release
```

It writes `./data/feed_state.json` relative to the working directory and creates the directory
when it is missing.

Run the checks CI runs:

```bash
just verify            # fmt, clippy, test
```

After changing a configuration key, rewrite what is generated from it:

```bash
just regenerate        # docs/config.contract.json and the Dockerfile LABEL region
```

## Configuration

Configuration is layered by [terrace-config](https://github.com/TimSchoenle/terrace-config).
Lowest precedence first:

1. The defaults compiled into the config structs.
2. TOML at `$NETCUP_OFFER_BOT_CONFIG`, defaulting to `./config.toml`. A file, or every `*.toml`
   directly inside it when it names a directory, merged in file-name order. A missing file is
   not an error.
3. `NETCUP_OFFER_BOT_`-prefixed environment variables.
4. Every key-named file in `$NETCUP_OFFER_BOT_SECRETS_DIR`.
5. `NETCUP_OFFER_BOT_<KEY>_FILE=/path`, which reads the value from that path.

The last three are mutually exclusive per key. Two of them supplying one key fails the boot,
naming the key and both sources.

Nesting is `__`, because a single underscore is part of a field name. Case is folded, so
`discord.webhook_url` is `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` as a variable and
`discord__webhook_url` as a file name.

| TOML | Type | Environment | Default | Flags | Purpose |
|---|---|---|---|---|---|
| `discord.webhook_url` | `SecretString` | `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` | — | required, secret | Discord webhook the offers are posted to. |
| `feed.check_interval_secs` | `u64` | `NETCUP_OFFER_BOT_FEED__CHECK_INTERVAL_SECS` | — | required | Seconds between two RSS feed checks. |
| `metrics.ip` | `IpAddr` | `NETCUP_OFFER_BOT_METRICS__IP` | `127.0.0.1` | — | Address the Prometheus exporter binds. `0.0.0.0` to reach it from outside the container. |
| `metrics.port` | `u16` | `NETCUP_OFFER_BOT_METRICS__PORT` | `9184` | — | Port the Prometheus exporter listens on. |
| `telemetry.log_level` | `Level` | `NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL` | `INFO` | — | The maximum verbosity that reaches stdout: `TRACE`, `DEBUG`, `INFO`, `WARN` or `ERROR`, in any case. |
| `telemetry.sentry_dsn` | `SecretString` | `NETCUP_OFFER_BOT_TELEMETRY__SENTRY_DSN` | unset | secret | Sentry DSN. Unset disables Sentry entirely. |

Run with `NETCUP_OFFER_BOT_TELEMETRY__LOG_LEVEL=DEBUG` and the boot log names the layer every
key was read from. That is what answers "the `Secret` is mounted and the bot is still posting to
the old webhook".

[docs/CONFIGURATION.md](docs/CONFIGURATION.md) has the rest: the two variables the loader reads
before any layer exists, both further spellings of every key, a `config.toml` carrying all of
them, the secrets-directory recipes, and the contract each image publishes.

## Operations

### Metrics

The Prometheus exporter serves `/metrics` on the address `metrics.ip` and `metrics.port` name,
`127.0.0.1:9184` unless configured otherwise. Bind `0.0.0.0` to reach it from outside the
container.

| Metric | Type | Labels | Reports |
| --- | --- | --- | --- |
| `feed_counter` | counter | `feed` | Items a round found to be new, counted before they are sent |
| `feed_fetch_duration_seconds` | histogram | `feed` | Seconds one fetch of the feed took |
| `feed_fetch_errors_total` | counter | `feed` | Fetches that failed, malformed payloads excluded |
| `webhook_errors_total` | counter | `feed` | Items still undelivered after the fifth attempt |

### State

`./data/feed_state.json` holds one UTC timestamp per feed and is rewritten only by a round that
moved one. The image's working directory is `/app`, so the file a deployment has to keep is
`/app/data/feed_state.json`.

### Runtime posture

The runtime stage is `FROM scratch`. It carries the stripped binary, the CA bundle, the zone
database, `/etc/passwd`, `/etc/group` and the offline copy of the configuration contract at
`/config/contract.json`. The process runs as `1001:1001` and writes to `/app/data` and stdout,
nothing else.

## Compatibility

| | Supported |
| --- | --- |
| Rust | edition 2024 |
| Platforms | `linux/amd64`, `linux/arm64` |
| Helm chart | [`timschoenle/netcup-offer-bot`](https://github.com/TimSchoenle/helm-charts/tree/main/charts/netcup-offer-bot) |

## Documentation

| Document | Purpose |
| --- | --- |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Every configuration key this service reads, in every spelling that can supply it. |
| [docs/config.contract.json](docs/config.contract.json) | — |

## Contributing

Issues and pull requests are welcome. Commit messages follow Conventional Commits, which is what
release-please reads to decide the next version, and `just verify` runs the checks a pull request
is going to run anyway.

Four things here are generated, and each names its source in its first lines: `README.md`,
`docs/CONFIGURATION.md`, `docs/config.contract.json` and the `LABEL` region of the `Dockerfile`.
`just regenerate` rewrites the last two. CI renders the first two, commits the result back to the
branch, and fails a push to master that does not match.

## Security

Do not open a public issue for a vulnerability. [SECURITY.md](SECURITY.md) has the reporting
route and the supported versions.

The Discord webhook is the one credential this process holds, and anyone holding it can post to
the channel. Supply it as a file rather than as an environment variable.

## License

`GPL-3.0-only`. [LICENSE](LICENSE) has the terms.
