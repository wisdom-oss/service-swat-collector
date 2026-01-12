FROM rust:alpine AS build-service

ARG RUST_BUILD_ARGS=""

RUN apk update && apk add ca-certificates && apk cache clean
RUN apk add musl-dev libc-dev
WORKDIR /build

# build only dependencies
COPY Cargo.toml /build/Cargo.toml
COPY Cargo.lock /build/Cargo.lock
RUN mkdir /build/src
RUN touch /build/src/lib.rs
RUN cargo build --release --locked --features health-check $RUST_BUILD_ARGS
RUN rm /build/src/lib.rs

# build application
COPY . /build
RUN cargo build --release --locked --features health-check $RUST_BUILD_ARGS

FROM alpine:latest
COPY --from=build-service /build/target/release/swat-collector /swat-collector
HEALTHCHECK --retries=0 CMD /swat-collector --health-check
ENTRYPOINT ["/swat-collector"]
LABEL org.opencontainers.image.source=https://github.com/wisdom-oss/service-swat-collector
# TODO: when this should handle request, this needs an exposed port
