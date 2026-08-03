FROM rust:1-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
RUN cargo build --locked --release --bin deriv_platform

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 platform && mkdir -p /var/lib/deriv-risk && chown platform /var/lib/deriv-risk
WORKDIR /app
COPY --from=build /app/target/release/deriv_platform /usr/local/bin/deriv_platform
COPY config ./config
ENV DERIV_RISK_STATE_DIR=/var/lib/deriv-risk
VOLUME ["/var/lib/deriv-risk"]
USER platform
EXPOSE 8080
ENTRYPOINT ["deriv_platform"]
