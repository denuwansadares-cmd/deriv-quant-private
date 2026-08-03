# Safe Product API

The dashboard is a demo-only shell. It cannot enable live trading, accept broker credentials, or submit buy/sell orders.

## Secure local run

The default bind address is `127.0.0.1:8080`. The status and qualification endpoints are read-only. Emergency stop requires:

- a 32+ character `DERIV_DASHBOARD_TOKEN`;
- a future `DERIV_DASHBOARD_TOKEN_EXPIRES_UNIX`;
- `Authorization: Bearer ...`;
- matching `X-CSRF-Token`;
- `X-Deriv-Role: operator`.

The endpoint is rate-limited and writes a local audit record. Stop clearing is deliberately unavailable through the API.

## Docker Compose

Set a temporary token and expiry, then run `docker compose up --build`. Compose binds the host port to loopback, mounts the persistent `deriv-risk` volume, drops Linux capabilities, enables no-new-privileges, and uses a read-only root filesystem.

The host used for this evidence package did not have a Docker engine; image execution remains a buyer verification step.

## Remote access

Do not expose the Rust HTTP listener directly to the internet. Use a maintained TLS reverse proxy, restrict source networks, rotate short-lived dashboard tokens, and retain reverse-proxy access logs. OAuth2 PKCE and multi-user RBAC are not implemented.

## REST endpoints

- `GET /api/v1/status`
- `GET /api/v1/qualifications`
- `POST /api/v1/emergency-stop` (authenticated operator only)
