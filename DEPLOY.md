# Nest Deployment & Hardening Guide

This document covers shipping the MVP: containerized Nest server, Windows Bird installer, and production hardening.

## 1. Docker image

The included [`Dockerfile`](./Dockerfile) builds the Nest server from the Cargo workspace and produces a `debian:bookworm-slim` image.

```bash
docker build -t nest-server:latest .
docker run -d \
  --name nest \
  -p 8140:8140 \
  -v nest-data:/data \
  -e NEST_TOKEN_SECRET=$(openssl rand -hex 32) \
  nest-server:latest
```

Environment variables (all optional; defaults shown):

| Variable | Default | Purpose |
|----------|---------|---------|
| `NEST_BIND_ADDR` | `0.0.0.0:8140` | Address/port the server binds to. |
| `NEST_DATA_DIR` | `data` | Root directory for the SQLite DB and Egg archives. |
| `NEST_DB_PATH` | `{NEST_DATA_DIR}/nest.sqlite` | SQLite file path. |
| `NEST_BROOD_LIMIT` | `10` | Max Eggs kept per game before pruning. |
| `NEST_TOKEN_SECRET` | `dev-insecure-secret-change-me` | HMAC key for auth tokens. **Change in production.** |
| `NEST_TOKEN_EXPIRY_SECONDS` | `604800` (7 days) | Token lifetime. |
| `NEST_LOG` | `info` | Tracing filter (`info`, `debug`, `nest=debug`). |

### Volume layout

The container stores everything under `/data` by default:

```
/data/
  nest.sqlite          # SQLite database
  flocks/
    {flock_id}/
      {game_id}/
        egg_{timestamp}_{egg_id}.zip
```

Always mount a persistent volume at `/data` so Eggs and the DB survive container restarts.

## 2. Pikapods deployment

1. Build or push the image to a registry of your choice.
2. In Pikapods, create a new pod from your image.
3. Add a persistent volume mounted at `/data`.
4. Set the required environment variables, especially `NEST_TOKEN_SECRET`.
5. Expose port `8140` and, if possible, place Pikapods' HTTPS edge in front of it.
6. Use the pod URL as the Nest URL in the Bird client.

A minimal `docker-compose.yml` for reference:

```yaml
services:
  nest:
    image: nest-server:latest
    ports:
      - "8140:8140"
    volumes:
      - nest-data:/data
    environment:
      NEST_TOKEN_SECRET: "change-me-to-a-long-random-value"
      NEST_LOG: "info"
    restart: unless-stopped
volumes:
  nest-data:
```

## 3. HTTPS / TLS and secret management

The Nest does not terminate TLS itself. Run it behind a reverse proxy or platform edge (Nginx, Caddy, Traefik, Pikapods) that provides HTTPS.

* Keep `NEST_TOKEN_SECRET` at least 32 bytes of random data (`openssl rand -hex 32`).
* Do not commit `.env` or secrets to Git.
* Rotate `NEST_TOKEN_SECRET` only when users are ready to log in again; existing tokens become invalid immediately.
* The server returns generic `401 Unauthorized` responses for missing/invalid tokens; internal errors are never leaked.

## 4. Windows Bird installer

The Tauri app is configured to build a `.msi` installer. On a Windows machine with the Rust/MSVC toolchain and WebView2:

```powershell
cd bird
cargo tauri build
```

The installer will be produced under `bird/src-tauri/target/release/bundle/msi/`.

For code signing, set the `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD` environment variables (Tauri signing), or update `tauri.conf.json`:

```json
"bundle": {
  "windows": {
    "certificateThumbprint": "<cert-thumbprint>",
    "timestampUrl": "http://timestamp.digicert.com"
  }
}
```

During development you can run `cargo tauri dev` from `bird/`.

## 5. Backup and restore

Back up two things:

1. The SQLite database (`nest.sqlite` or `NEST_DB_PATH`).
2. The `/data/flocks/` archive directory.

A simple backup script:

```bash
#!/bin/bash
DEST=/backup/nest-$(date +%Y%m%d-%H%M%S)
mkdir -p "$DEST"
cp -r /data/flocks "$DEST/"
cp /data/nest.sqlite "$DEST/"
```

Restore by stopping the container, copying the backup back to `/data`, and restarting.

## 6. Security hardening

* **Authorization:** Every API route except `/health`, `/api/flock/register`, and `/api/flock/login` requires a valid `Authorization: Bearer <token>` header and enforces Flock scoping in the repository layer.
* **Input validation:** `game_id` values are validated and sanitized before use; path separators and null bytes are rejected, and the on-disk directory name is a safe slug.
* **Upload limits:** `RequestBodyLimit` caps the request body at 50 MB; larger uploads are rejected with `413 Payload Too Large`.
* **Path traversal:** `egg_id` values are parsed as UUIDs, never user strings, and archive paths are resolved relative to the configured `data_dir`.
* **Passwords:** Argon2id is used for password hashing.
* **Dependency audit:** Run `cargo audit` regularly:

```bash
cargo install cargo-audit
cargo audit
```

## 7. Observability

* Request logging and timing are provided by `tower_http::trace::TraceLayer`.
* Set `NEST_LOG=debug` for more verbose output.
* The `/health` endpoint can be used for load-balancer health checks.

## 8. Performance / RAM target

The Nest is designed to stay under 30 MB of RAM under light load. If you need to profile:

```bash
cargo run --bin nest --release
# or in a container:
docker stats <container>
```

If memory grows, check the SQLite connection pool (`max_connections = 5`) and the size of queued Egg uploads on the Bird side.
