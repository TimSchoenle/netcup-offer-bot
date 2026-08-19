# syntax=docker/dockerfile:1.26@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32

# Global Build Args
ARG BINARY_NAME=netcup-offer-bot
ARG USER_ID=1001
ARG GROUP_ID=1001
ARG EXECUTION_DIRECTORY=/app

FROM lukemathwalker/cargo-chef:latest-rust-alpine AS chef-base
ARG EXECUTION_DIRECTORY

RUN apk add --no-cache \
    curl \
    jq \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig \
    upx

WORKDIR ${EXECUTION_DIRECTORY}

# Per-architecture build parameters.
#
# Resolving them through stage names rather than a shell case makes an unsupported
# TARGETARCH fail the build immediately ("stage not found") instead of silently
# producing an artifact for the wrong target.
#
# The image is built natively on a runner of the matching architecture, so
# BUILD_TARGET is always the host triple and no cross toolchain is required.
#
# UPX_ENABLED is off on arm64: the UPX stub assumes a 4 KiB kernel page size, so
# packed arm64 binaries fail to start on 64 KiB-page kernels (RHEL/CentOS arm64
# and similar). The few megabytes saved are not worth an image that cannot run.
FROM chef-base AS chef-amd64
ENV BUILD_TARGET=x86_64-unknown-linux-musl \
    SENTRY_CLI_ARCH=x86_64 \
    UPX_ENABLED=1

FROM chef-base AS chef-arm64
ENV BUILD_TARGET=aarch64-unknown-linux-musl \
    SENTRY_CLI_ARCH=aarch64 \
    UPX_ENABLED=0

FROM chef-${TARGETARCH} AS chef

RUN LATEST_VERSION=$(curl -s https://api.github.com/repos/getsentry/sentry-cli/releases/latest | jq -r .tag_name) && \
    curl -fsSL "https://downloads.sentry-cdn.com/sentry-cli/${LATEST_VERSION}/sentry-cli-Linux-${SENTRY_CLI_ARCH}" -o /usr/local/bin/sentry-cli && \
    chmod +x /usr/local/bin/sentry-cli

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ARG EXECUTION_DIRECTORY
ARG BINARY_NAME
ARG TARGETARCH

COPY --from=planner /app/recipe.json recipe.json

# Cache mounts are scoped per architecture so that multi-platform builds sharing a
# single builder cannot overwrite each other's artifacts.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETARCH} \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETARCH} \
    cargo chef cook --release --target ${BUILD_TARGET} --recipe-path recipe.json

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETARCH} \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-${TARGETARCH} \
    --mount=type=cache,target=${EXECUTION_DIRECTORY}/target,id=cargo-target-${TARGETARCH} \
    cargo build --release --target ${BUILD_TARGET} && \
    cp ${EXECUTION_DIRECTORY}/target/${BUILD_TARGET}/release/${BINARY_NAME} /tmp/${BINARY_NAME}

# Upload debug symbols to Sentry before stripping.
#
# The upload reads /tmp/${BINARY_NAME}: the build output under target/ lives on a
# cache mount and is no longer present once the previous RUN has finished.
ARG SENTRY_ORG
ARG SENTRY_PROJECT
ARG VERSION

RUN --mount=type=secret,id=sentry_token,env=SENTRY_AUTH_TOKEN \
    sh -eu -c '\
      if [ -n "${SENTRY_AUTH_TOKEN:-}" ] && [ -n "${SENTRY_ORG:-}" ] && [ -n "${SENTRY_PROJECT:-}" ]; then \
        echo "Uploading debug files for ${BUILD_TARGET} to Sentry." ; \
        sentry-cli debug-files upload \
          --auth-token "${SENTRY_AUTH_TOKEN}" \
          --org "${SENTRY_ORG}" \
          --project "${SENTRY_PROJECT}" \
          --include-sources \
          "/tmp/${BINARY_NAME}" ; \
      else \
        echo "Skipping Sentry upload (missing token and/or org/project args)" ; \
      fi \
    '

# Strip and compress after uploading symbols
RUN strip --strip-all /tmp/${BINARY_NAME} && \
    if [ "${UPX_ENABLED}" = "1" ]; then \
      upx --best --lzma /tmp/${BINARY_NAME} ; \
    else \
      echo "Skipping UPX compression for ${BUILD_TARGET}" ; \
    fi

# The configuration contract, and the labels that make it discoverable.
#
# `FROM chef` rather than `FROM builder`: the generator reads the same source tree and wants the
# same toolchain, but nothing it emits depends on the release binary having linked. Basing it on
# `builder` would queue a document that takes seconds behind a fat-LTO musl build that takes
# minutes — and would make the host-side export in CI drag that whole build along with it.
#
# Both files come out of one stage, from one source tree, in one build. That is the entire
# guarantee: the `LABEL` block in `runtime` below is checked against `contract.labels` after the
# image exists, and a document generated at some other time could disagree with it.
FROM chef AS contract-builder

ARG EXECUTION_DIRECTORY
ARG TARGETARCH
ARG VERSION

COPY . .

# Caches of its own, sharing none of `builder`'s. This stage depends on nothing `builder`
# produces, so BuildKit runs the two concurrently — and a BuildKit cache mount is `sharing=shared`
# by default, which put two cargo processes in one `$CARGO_HOME/git` with a `.package-cache` lock
# each. The result was `failed to clone into … no error`, non-deterministically, on whichever of
# the two lost. Separate ids remove the contention rather than serialising the two heaviest
# stages behind `sharing=locked`, and cost nothing in CI, where cache mounts are not exported and
# start empty either way.
#
# The target cache would have had to be separate regardless: this is a host-target debug build of
# an example and shares no artefacts with a `--release --target *-musl` one.
#
# `--version` is passed only when the build was told a release. Absent, the document simply omits
# the field, which is what keeps the committed copy in `docs/` stable across a version bump and
# so keeps the drift gate from failing every release pull request.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-contract-${TARGETARCH} \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git-contract-${TARGETARCH} \
    --mount=type=cache,target=${EXECUTION_DIRECTORY}/target,id=cargo-target-contract-${TARGETARCH} \
    sh -eu -c '\
      mkdir -p /out ; \
      if [ -n "${VERSION:-}" ] ; then set -- --version "${VERSION}" ; else set -- ; fi ; \
      cargo run --quiet --features config-schema --example config-contract \
        -- --format contract "$@" > /out/contract.json ; \
      cargo run --quiet --features config-schema --example config-contract \
        -- --format labels   "$@" > /out/contract.labels \
    '

# The two generated files and nothing else, so `--output type=local` puts them on the host
# without unpacking the builder's toolchain alongside them.
#
#   docker buildx build --target contract-export --output type=local,dest=./contract-out .
FROM scratch AS contract-export
COPY --from=contract-builder /out/ /

FROM alpine:3.24@sha256:28bd5fe8b56d1bd048e5babf5b10710ebe0bae67db86916198a6eec434943f8b AS env
ARG USER_ID

# mailcap is used for content type (MIME type) detection
# tzdata is used for timezones info
RUN apk add --no-cache \
    ca-certificates \
    mailcap \
    tzdata && \
    update-ca-certificates && \
    adduser \
        --disabled-password \
        --gecos "" \
        --home "/nonexistent" \
        --shell "/sbin/nologin" \
        --no-create-home \
        --uid "${USER_ID}" \
        "appuser"

FROM scratch AS runtime

# Build Environment Args
ARG BINARY_NAME
ARG USER_ID
ARG GROUP_ID
ARG EXECUTION_DIRECTORY

ARG version=unknown
ARG release=unreleased

LABEL version=${version} \
      release=${release}

# Emitted verbatim by:
#
#   cargo run --features config-schema --example config-contract -- --format dockerfile
#
# Pasted, never retyped, and never hand-edited: a `LABEL` key cannot be interpolated from
# anything, so what makes these trustworthy is not how they were written but that CI compares the
# built image's config blob against `contract.labels` from the same generator run — on every
# platform — before anything is attached to the digest or signed.

# The markers are the crate's own and are what the drift check cuts on. A substring test over
# the whole file — what this repository did before — passes an image whose block was pasted
# twice, and cutting by line count passes one that lost a label.
# terrace-config:labels:begin
LABEL dev.terrace.config.contract.version="1" \
      dev.terrace.config.contract.path="/config/contract.json" \
      dev.terrace.config.prefix="NETCUP_OFFER_BOT_"
# terrace-config:labels:end

COPY --from=env /etc/passwd /etc/passwd
COPY --from=env /etc/group /etc/group
COPY --from=env /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=env /usr/share/zoneinfo /usr/share/zoneinfo

# The offline copy of the contract, at the path `dev.terrace.config.contract.path` above names.
# Read by anything holding the image and no registry — `docker save`, `crane export`, an
# air-gapped mirror. The canonical copy is the OCI referrer attached to the pushed digest; this
# one costs a few kilobytes and removes the registry from the dependency list.
COPY --from=contract-builder /out/contract.json /config/contract.json

WORKDIR ${EXECUTION_DIRECTORY}
COPY --from=builder --chmod=555 /tmp/${BINARY_NAME} ./app

USER ${USER_ID}:${GROUP_ID}

ENTRYPOINT ["./app"]
