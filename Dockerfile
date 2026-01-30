# syntax=docker/dockerfile:1.20@sha256:26147acbda4f14c5add9946e2fd2ed543fc402884fd75146bd342a7f6271dc1d

# Global Build Args
ARG BINARY_NAME=netcup-offer-bot
ARG USER=runner
ARG GROUP=runner
ARG USER_ID=1000
ARG GROUP_ID=1000
ARG EXECUTION_DIRECTORY=/app
ARG BUILD_DIRECTORY=/build
ARG BUILD_TARGET=x86_64-unknown-linux-musl

FROM lukemathwalker/cargo-chef:latest-rust-alpine AS chef
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static upx curl jq
# Install sentry-cli
RUN LATEST_VERSION=$(curl -s https://api.github.com/repos/getsentry/sentry-cli/releases/latest | jq -r .tag_name) && \
    wget -qO /usr/local/bin/sentry-cli "https://downloads.sentry-cdn.com/sentry-cli/${LATEST_VERSION}/sentry-cli-Linux-x86_64" && \
    chmod +x /usr/local/bin/sentry-cli
WORKDIR /app

FROM chef AS planner
COPY  . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ARG BUILD_TARGET
ARG BUILD_DIRECTORY
ARG BUILD_TARGET
ARG BINARY_NAME

COPY --from=planner  /app/recipe.json recipe.json
RUN cargo chef cook --release --target ${BUILD_TARGET} --recipe-path recipe.json

COPY  . .

RUN cargo build --release --target ${BUILD_TARGET}

# Upload debug symbols to Sentry before stripping
ARG SENTRY_ORG
ARG SENTRY_PROJECT
ARG VERSION

RUN --mount=type=secret,id=sentry_token \
    if [ -f /run/secrets/sentry_token ]; then \
        sentry-cli debug-files upload \
            --auth-token $(cat /run/secrets/sentry_token) \
            --org ${SENTRY_ORG} \
            --project ${SENTRY_PROJECT} \
            --include-sources \
            $BUILD_DIRECTORY/target/$BUILD_TARGET/release/$BINARY_NAME; \
    fi

# Strip and compress after uploading symbols
RUN strip --strip-all $BUILD_DIRECTORY/target/$BUILD_TARGET/release/$BINARY_NAME && \
    upx --best --lzma $BUILD_DIRECTORY/target/$BUILD_TARGET/release/$BINARY_NAME

FROM alpine:3.23@sha256:865b95f46d98cf867a156fe4a135ad3fe50d2056aa3f25ed31662dff6da4eb62 AS env

# mailcap is used for content type (MIME type) detection
# tzdata is used for timezones info
RUN apk update && \
    apk upgrade --no-cache && \
    apk add --no-cache ca-certificates mailcap tzdata

RUN update-ca-certificates

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "10001" \
    "appuser"

FROM scratch AS runtime

# Build Environment Args
ARG BINARY_NAME
ARG USER
ARG GROUP
ARG EXECUTION_DIRECTORY
ARG BUILD_DIRECTORY
ARG BUILD_TARGET

ARG version=unknown
ARG release=unreleased

LABEL version=${version} \
      release=${release}

COPY --from=env  /etc/passwd /etc/passwd
COPY --from=env  /etc/group /etc/group
COPY --from=env  /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=env  /usr/share/zoneinfo /usr/share/zoneinfo

# Create execution directory
COPY --from=env --chown=$USER:$GROUP $EXECUTION_DIRECTORY $EXECUTION_DIRECTORY

WORKDIR $EXECUTION_DIRECTORY
COPY --from=builder --chown=root:root $BUILD_DIRECTORY/target/$BUILD_TARGET/release/$BINARY_NAME $EXECUTION_DIRECTORY

USER $USER:$GROUP

ENTRYPOINT ["./app"]