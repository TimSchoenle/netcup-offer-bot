# syntax=docker/dockerfile:1.21@sha256:27f9262d43452075f3c410287a2c43f5ef1bf7ec2bb06e8c9eeb1b8d453087bc

# Global Build Args
ARG BINARY_NAME=netcup-offer-bot
ARG USER_ID=1001
ARG GROUP_ID=1001
ARG EXECUTION_DIRECTORY=/app
ARG BUILD_TARGET=x86_64-unknown-linux-musl

FROM lukemathwalker/cargo-chef:latest-rust-alpine AS chef
ARG EXECUTION_DIRECTORY

RUN apk add --no-cache \
    curl \
    jq \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig \
    upx && \
    LATEST_VERSION=$(curl -s https://api.github.com/repos/getsentry/sentry-cli/releases/latest | jq -r .tag_name) && \
    curl -fsSL "https://downloads.sentry-cdn.com/sentry-cli/${LATEST_VERSION}/sentry-cli-Linux-x86_64" -o /usr/local/bin/sentry-cli && \
    chmod +x /usr/local/bin/sentry-cli

WORKDIR ${EXECUTION_DIRECTORY}

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ARG BUILD_TARGET
ARG EXECUTION_DIRECTORY
ARG BINARY_NAME

COPY --from=planner /app/recipe.json recipe.json

# Use cargo cache mount for faster dependency builds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --target ${BUILD_TARGET} --recipe-path recipe.json

COPY . .

# Use cargo cache and target cache for faster builds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=${EXECUTION_DIRECTORY}/target \
    cargo build --release --target ${BUILD_TARGET} && \
    cp ${EXECUTION_DIRECTORY}/target/${BUILD_TARGET}/release/${BINARY_NAME} /tmp/${BINARY_NAME}

# Upload debug symbols to Sentry before stripping
ARG SENTRY_ORG
ARG SENTRY_PROJECT
ARG VERSION

RUN --mount=type=secret,id=sentry_token,env=SENTRY_AUTH_TOKEN \
    sh -eu -c '\
      if [ -n "${SENTRY_AUTH_TOKEN:-}" ] && [ -n "${SENTRY_ORG:-}" ] && [ -n "${SENTRY_PROJECT:-}" ]; then \
        echo "Uploading source map to Sentry." ; \
        sentry-cli debug-files upload \
          --auth-token "${SENTRY_AUTH_TOKEN}" \
          --org "${SENTRY_ORG}" \
          --project "${SENTRY_PROJECT}" \
          --include-sources \
          "${EXECUTION_DIRECTORY}/target/${BUILD_TARGET}/release/${BINARY_NAME}" ; \
      else \
        echo "Skipping Sentry upload (missing token and/or org/project args)" ; \
      fi \
    '

# Strip and compress after uploading symbols
RUN strip --strip-all /tmp/${BINARY_NAME} && \
    upx --best --lzma /tmp/${BINARY_NAME}

FROM alpine:3.23@sha256:25109184c71bdad752c8312a8623239686a9a2071e8825f20acb8f2198c3f659 AS env
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

COPY --from=env /etc/passwd /etc/passwd
COPY --from=env /etc/group /etc/group
COPY --from=env /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=env /usr/share/zoneinfo /usr/share/zoneinfo

WORKDIR ${EXECUTION_DIRECTORY}
COPY --from=builder --chmod=555 /tmp/${BINARY_NAME} ./app

USER ${USER_ID}:${GROUP_ID}

ENTRYPOINT ["./app"]
