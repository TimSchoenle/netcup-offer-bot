# Global Build Args
ARG BINARY_NAME=netcup-offer-bot
ARG USER=runner
ARG GROUP=runner
ARG USER_ID=1000
ARG GROUP_ID=1000
ARG EXECUTION_DIRECTORY=/app
ARG BUILD_DIRECTORY=/build
ARG BUILD_TARGET=x86_64-unknown-linux-musl

FROM clux/muslrust:stable@sha256:cd914f6f4d398329cd62c4b7fbeeef72390e9322dd51631ab9acdca6abb7046e AS chef

# Build Environment Args
ARG BUILD_DIRECTORY

USER root
RUN cargo install cargo-chef
WORKDIR $BUILD_DIRECTORY

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ARG BUILD_DIRECTORY
ARG BUILD_TARGET

COPY --from=planner $BUILD_DIRECTORY/recipe.json recipe.json
RUN cargo chef cook --release --target $BUILD_TARGET --recipe-path recipe.json
COPY . .
RUN cargo build --release --target $BUILD_TARGET

FROM alpine@sha256:25109184c71bdad752c8312a8623239686a9a2071e8825f20acb8f2198c3f659 AS env

# Build Environment Args
ARG USER
ARG GROUP
ARG USER_ID
ARG GROUP_ID
ARG EXECUTION_DIRECTORY

RUN apk add --no-cache ca-certificates && \
    addgroup -g $GROUP_ID -S $GROUP &&  \
    adduser -u $USER_ID -S $USER -G $GROUP && \
    mkdir -p $EXECUTION_DIRECTORY

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

COPY --from=env --chown=root:root /etc/passwd /etc/passwd
COPY --from=env --chown=root:root /etc/group /etc/group
COPY --from=env --chown=root:root /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Create execution directory
COPY --from=env --chown=$USER:$GROUP $EXECUTION_DIRECTORY $EXECUTION_DIRECTORY

WORKDIR $EXECUTION_DIRECTORY
COPY --from=builder --chown=root:root $BUILD_DIRECTORY/target/$BUILD_TARGET/release/$BINARY_NAME ./app

USER $USER:$GROUP

ENTRYPOINT ["./app"]