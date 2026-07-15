# Nest  

Monorepo for the Nest project. A cross-platform game backup solution.

/!\ This project is a proof of concept and pretty much fully vibe coded (for now), please use it at your own risk /!\

## Specs

See [SPECS.md](./SPECS.md)

## Todo

See [TODO.md](./TODO.md)

## Development

This is a Cargo workspace:

- `nest/` — **The Nest** server crate (binary `nest`): `axum` HTTP server + SQLite.
- `shared/` — `nest-shared`: transport-agnostic domain models shared across crates.
- `bird/` — the Tauri client (added in a later phase).

### Prerequisites

- A Rust toolchain (see [`rust-toolchain.toml`](./rust-toolchain.toml); `rustup` auto-installs it).
- On Windows, the MSVC C++ build tools (for linking SQLite).

### Common commands

```sh
cargo build                 # build the workspace
cargo test --all            # run unit + integration tests
cargo fmt --all --check     # formatting check
cargo clippy --all-targets --all-features -- -D warnings   # lints

cargo run --bin nest        # run the Nest server
```

### Running the Nest server

Configuration is read from environment variables (all optional; see
[`.env.example`](./.env.example)). By default the server binds `127.0.0.1:8140`,
stores its SQLite DB and Eggs under `./data`, issues HMAC-SHA256 signed tokens
using `NEST_TOKEN_SECRET`, and expires them after 7 days.

```sh
cargo run --bin nest

# Health check
curl http://127.0.0.1:8140/health

# Authentication
curl -X POST http://127.0.0.1:8140/api/flock/register \
  -H 'Content-Type: application/json' \
  -d '{"username":"valley","password":"correct-horse-battery-staple"}'

curl -X POST http://127.0.0.1:8140/api/flock/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"valley","password":"correct-horse-battery-staple"}'

# Device management (replace $TOKEN with the value returned above)
curl -X POST http://127.0.0.1:8140/api/birds \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"name":"Main Desktop","platform":"windows"}'

curl http://127.0.0.1:8140/api/birds \
  -H "Authorization: Bearer $TOKEN"
```

### Pre-commit hooks

The repo ships a [`.pre-commit-config.yaml`](./.pre-commit-config.yaml) that runs
`cargo fmt` and `cargo clippy` before each commit. Enable it once with:

```sh
pip install pre-commit   # or: pipx install pre-commit
pre-commit install
```

## License

[MIT](./LICENSE)
