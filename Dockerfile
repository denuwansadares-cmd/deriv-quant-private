FROM rust:1-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
COPY config ./config
RUN cargo build --locked --release --bins

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home platform \
    && mkdir -p /var/lib/deriv-risk /var/lib/deriv-audit \
    && chown -R platform:platform /var/lib/deriv-risk /var/lib/deriv-audit
WORKDIR /app
COPY --from=build /app/target/release/deriv_bot /usr/local/bin/deriv_bot
COPY --from=build /app/target/release/deriv_watchdog /usr/local/bin/deriv_watchdog
COPY --from=build /app/target/release/deriv_platform /usr/local/bin/deriv_platform
COPY config ./config
ENV DERIV_RISK_STATE_DIR=/var/lib/deriv-risk \
    DERIV_AUDIT_JOURNAL_DIR=/var/lib/deriv-audit \
    HISTORY_PATH=/history/normalized
USER 10001:10001
