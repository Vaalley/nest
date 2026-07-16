# Nest — Roadmap & Implementation Plan

This document breaks the work described in [SPECS.md](./SPECS.md) into ordered,
testable phases. It covers the two deliverables of the monorepo:

- **The Nest** — a lightweight Rust + SQLite server (target: < 30 MB RAM, deployable to Pikapods).
- **The Bird** — a Tauri client (Rust backend + web frontend) that scans games, watches play sessions, and packages saves ("Eggs").

Domain glossary (from the spec):

- **Flock** — a user account.
- **Bird** — a registered client device.
- **Egg** — a single zipped save-file snapshot.
- **Clutch** — the rolling collection of Eggs for one game.
- **Brood Limit** — max Eggs kept per Clutch before the oldest are pruned (default: 10).

Legend: `[ ]` todo · `[~]` in progress · `[x]` done. Each phase lists **Goals**,
**Tasks**, and an **Exit criteria** checkpoint that must pass before moving on.

---

## Phase 0 — Monorepo Foundation & Tooling

**Goal:** A clean, reproducible monorepo skeleton with shared tooling and CI so
every later phase builds on stable ground.

- [x] Decide monorepo layout: `nest/` (server crate), `bird/` (Tauri app), `shared/` (crate for common DTOs, error types, hashing, domain enums). _(`nest/` + `shared/` created; `bird/` deferred to Phase 6.)_
- [x] Set up a Cargo workspace at the repo root (`Cargo.toml` with `members`).
- [x] Add `rust-toolchain.toml` pinning the toolchain; enable `clippy` and `rustfmt`.
- [x] Add `.editorconfig`, `.gitignore` (Rust `target/`, Tauri build output, `.env`, `/data`).
- [x] Add `rustfmt.toml` and `clippy.toml`; agree on lint policy (`-D warnings` in CI).
- [x] Set up pre-commit hooks (`.pre-commit-config.yaml` or a Husky-style hook) running `cargo fmt --check` + `cargo clippy`. _(`.pre-commit-config.yaml` local hooks.)_
- [x] GitHub Actions CI: matrix build (Linux + Windows), `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`. _(`.github/workflows/ci.yml`.)_
- [x] Choose and document core dependencies (web framework e.g. `axum`, `sqlx` or `rusqlite`, `serde`, `tokio`, `tracing`, `argon2`, `jsonwebtoken`, `zip`, `blake3`/`sha2`). _(`axum`, `sqlx`+`sqlite`, `serde`, `tokio`, `tracing`, `thiserror`, `time`, `uuid`, `argon2`, `hmac`, `sha2`, `base64`, `rand`.)_
- [x] Root `README` updates: dev setup, build, run instructions per crate. _(Development section in `README.md` + `.env.example`.)_

**Exit criteria:** `cargo build`, `cargo fmt --check`, `cargo clippy`, and `cargo test` all pass locally and in CI on Linux + Windows. _(All pass locally on Windows; CI workflow added to run the matrix on Linux + Windows.)_

---

## Phase 1 — Nest Server: Skeleton & Data Layer

**Goal:** A runnable HTTP server with an embedded SQLite database, migrations,
config, logging, and health check.

- [x] Scaffold the `nest` server crate with the chosen web framework (`axum` + `tokio`).
- [x] Configuration loader (env vars + optional file): bind address, data dir, DB path, brood limit default, token secret. _(`nest/src/config.rs`; see `.env.example`.)_
- [x] Structured logging/tracing with configurable level. _(`tracing` + `tracing-subscriber` env-filter, `NEST_LOG`.)_
- [x] SQLite integration with a migration system (`sqlx migrate` or `refinery`). _(`sqlx` SQLite pool + embedded `migrate!`.)_
- [x] Initial schema migrations:
  - [x] `flocks` (id, username, password_hash, created_at).
  - [x] `birds` (id, flock_id, name, platform, last_seen, created_at).
  - [x] `clutches` (id, flock_id, game_id, brood_limit, created_at).
  - [x] `eggs` (id, clutch_id, source_bird_id, file_hash, size_bytes, file_path, created_at).
- [x] Domain models (DDD) in `shared/`: `Flock`, `Bird`, `Egg`, `Clutch` + status enums. _(`Platform`, `SyncStatus` enums in `nest-shared`.)_
- [x] Repository layer abstracting DB access per aggregate. _(`FlockRepository`, `BirdRepository`, `ClutchRepository`, `EggRepository`.)_
- [x] `GET /health` endpoint + graceful shutdown. _(DB-readiness check; Ctrl-C / SIGTERM shutdown.)_
- [x] Consistent JSON error envelope and `AppError` type mapped to HTTP status codes. _(`{"error":{"code","message"}}`; internal details never leaked.)_

**Exit criteria:** Server boots, applies migrations to a fresh SQLite file, `GET /health` returns `200`, RAM footprint sanity-checked. _(Done: `/health` returns 200 on a fresh DB; working set ≈ 9 MB, well under the 30 MB target.)_

---

## Phase 2 — Flock Management (Authentication)

**Goal:** Secure single-user account registration and login with token issuance.

- [x] `POST /api/flock/register` — validate username, hash password with Argon2, persist Flock.
- [x] `POST /api/flock/login` — verify credentials, issue a signed HMAC-SHA256 token (compact JWT-like format).
- [x] Auth middleware/extractor that validates the token and injects the current Flock into request context.
- [x] Reject duplicate usernames; rate-limit or throttle login attempts (basic per-IP sliding window, 5 req/min).
- [x] Input validation + uniform auth error responses (no user-enumeration leaks on login).
- [x] Tests for hashing/verification; integration tests for register → login → authenticated request.

**Exit criteria:** A new account can register, log in, receive a token, and use it to access a protected route; invalid credentials are rejected. _(Done.)_

---

## Phase 3 — Bird Management (Devices)

**Goal:** Registration and listing of client devices belonging to a Flock.

- [x] `POST /api/birds/register` — link a device (name, platform); associate with authenticated Flock; return Bird id/credentials (signed device token).
- [x] `GET /api/birds` — list the Flock's registered devices with `last_seen` status.
- [x] Update `last_seen` on authenticated device activity (set at registration, refreshed by the auth extractor).
- [x] Scope all Bird queries to the owning Flock (authorization checks via token claims).
- [x] Integration tests: register two Birds, list them, ensure cross-Flock isolation.

**Exit criteria:** A logged-in Flock can register and enumerate its Birds; Birds from other Flocks are never visible. _(Done.)_

---

## Phase 4 — Clutch & Egg Storage (Save Archives)

**Goal:** The core save-archive lifecycle: upload, list, download, delete, and prune.

- [x] Storage layout on disk: `/data/flocks/{user_id}/{game_id}/egg_[timestamp]_{egg_id}.zip`.
- [x] `POST /api/clutches/{game_id}/lay` — multipart upload (`.zip` + file hash + source Bird id); verify hash matches payload; create Clutch on first Egg; store file; insert Egg row.
- [x] `GET /api/clutches` — list a Flock's tracked games and current status.
- [x] `GET /api/clutches/{game_id}/eggs` — Egg metadata / version history for a Clutch.
- [x] `GET /api/clutches/{game_id}/hatch/{egg_id}` — stream a specific Egg back for restore.
- [x] `DELETE /api/clutches/{game_id}/eggs/{egg_id}` — remove an Egg (DB row + file).
- [x] **Brood Limit** enforcement: after `lay`, prune the oldest Eggs beyond the limit (default 10, user-configurable per Clutch); delete both row and file atomically.
- [x] Handle partial-upload failures and orphaned files (cleanup / transactional guarantees).
- [x] Integration tests: lay 12 Eggs with limit 10 → exactly 10 newest remain; hatch returns correct bytes; delete works.

**Exit criteria:** Full CRUD over Eggs works end-to-end, files land in the documented directory structure, and the Brood Limit prunes correctly.

---

## Phase 5 — Sync Coordination & Conflict Model (Server Side)

**Goal:** Server-side primitives that power "The Flight Home" — hash/timestamp
comparison and conflict signalling.

- [x] Pre-launch comparison endpoint/logic: given a game_id + local hash + timestamp, report whether the Nest has a newer Egg, an identical Egg, or a conflict.
- [x] Define Clutch/Egg status semantics: **Safe in Nest** (synced), **Flying** (syncing), **Chilly Egg** (conflict).
- [x] Conflict detection ("Chilly Egg"): both local and remote modified since last common ancestor.
- [x] Conflict resolution endpoint: choose local vs. Nest version ("which egg to keep warm").
- [x] Persist last-known-synced state per Bird per Clutch to detect divergence.
- [x] Tests covering: newer-remote pull, up-to-date no-op, and conflict paths.

**Exit criteria:** Given crafted hash/timestamp inputs, the server correctly classifies pull / no-op / conflict and supports an explicit resolution choice.

---

## Phase 6 — Bird Client: Tauri Scaffold

**Goal:** A minimal Tauri app that starts, talks to the Nest, and holds config.

- [x] Scaffold the `bird` Tauri app (Rust backend + web frontend of choice).
- [x] Local config/state store: Nest server URL, auth token, registered Bird id.
- [x] Typed API client (in `shared/` where possible) for all Nest endpoints.
- [x] Onboarding flow: log in / register against a Nest, then register this device as a Bird.
- [x] Secure local token storage (OS keychain or encrypted local file).
- [x] System-tray presence with a basic menu (open UI, quit).

**Exit criteria:** The client launches, authenticates against a running Nest, registers itself as a Bird, and shows in `GET /api/birds`. _(Done: Tauri v2 scaffold builds and tests pass; config, auth, and tray modules are implemented and the binary compiles on Windows.)

---

## Phase 7 — Foraging Engine (Ludusavi Integration)

**Goal:** Automatically discover where installed games store local saves.

- [x] Integrate the open-source Ludusavi manifest: fetch, cache, and refresh it.
- [x] Detect installed games and resolve their save paths from the manifest.
- [x] Map discovered games to stable `game_id`s usable by the Nest API.
- [x] Compute local save hashes + timestamps for comparison with the Nest.
- [x] Start with a curated subset of manually verified test games (MVP requirement) with a path to full-manifest coverage.
- [x] Handle games not in the manifest / missing save folders gracefully.

**Exit criteria:** For the verified test games, the Bird lists each game with its resolved local save path, hash, and last-modified time. _(Done: `ForagingEngine` fetches and caches the Ludusavi manifest, resolves save paths for the built-in verified subset using common placeholders, and computes directory hashes and mtimes; unit tests pass.)

---

## Phase 8 — Feather Agent (Process Monitoring)

**Goal:** Detect game launch/exit reliably with negligible resource use.

- [ ] Background worker that detects when a tracked game launches.
- [ ] While a game runs, sleep and monitor only the game's process PID (low CPU).
- [ ] Detect exit, then wait 5 seconds for final disk writes before acting.
- [ ] Windows process-monitoring implementation first (MVP target platform).
- [ ] Abstraction layer so Linux/SteamOS + macOS backends can be added later.
- [ ] Emit lifecycle events (launched / running / exited) into the sync engine.

**Exit criteria:** Launching and quitting a verified test game reliably fires launch/exit events with the 5-second post-exit delay, using minimal CPU while the game runs.

---

## Phase 9 — Egg Packaging & The Flight Home (Client Sync Cycle)

**Goal:** Wire foraging + agent + API into the full sync lifecycle from SPECS §4.

- [ ] **Leaving the Branch (pre-launch):** on game start, compare local hash/timestamp with the latest Egg via the Nest.
- [ ] **Hatching the Egg (pull):** if the Nest has a newer Egg, download, unzip into the save location, then allow launch.
- [ ] **Chilly Egg conflict:** if both sides changed offline, pause and prompt the user to choose which save to keep.
- [ ] **In Flight:** background-monitor the PID while the game runs.
- [ ] **Laying a New Egg (post-exit):** after the 5s delay, zip the updated save folder, compute its hash, and upload it via `lay` with the source Bird id.
- [ ] Retry/queue uploads when the Nest is unreachable; resume on reconnect.
- [ ] End-to-end test: play → exit → Egg uploaded → second Bird pulls the newer Egg.

**Exit criteria:** A complete round-trip works: a save modified on one Bird is packaged, uploaded, and correctly pulled onto a second Bird, with conflicts surfaced rather than silently overwritten.

---

## Phase 10 — Cozy UI ("The Branch")

**Goal:** A friendly interface to browse games and control syncing.

- [ ] Game list view: installed games with a "Keep Safe in the Nest" toggle per game.
- [ ] Status indicators per game: **Safe in Nest**, **Flying**, **Chilly Egg**.
- [ ] Egg/version-history view per game (from `GET /clutches/{game_id}/eggs`) with restore ("hatch") and delete actions.
- [ ] Conflict resolution dialog: *"This egg got cold while you were away. Which one do you want to keep warm?"*
- [ ] Manual "sync now" / "scan now" actions and clear progress/error feedback.
- [ ] Tray + window UX polish; empty/error/loading states.

**Exit criteria:** A user can toggle games for protection, see accurate live status, browse and restore version history, and resolve a conflict entirely from the UI.

---

## Phase 11 — Packaging, Deployment & Hardening (MVP Ship)

**Goal:** Ship the MVP: containerized Nest + a Windows Bird build.

- [ ] Dockerfile for the Nest (small base image, static/minimal build) with a persistent `/data` volume.
- [ ] Verify the < 30 MB RAM footprint target under light load; profile and trim if needed.
- [ ] Deployment docs for Pikapods (env vars, volume, port) and generic Docker hosts.
- [ ] HTTPS/TLS guidance (reverse proxy or built-in) and secret management for the token key.
- [ ] Windows installer/build for the Bird (Tauri bundling), code-signing notes.
- [ ] Backup/restore guidance for the SQLite DB + `/data` archive directory.
- [ ] Security hardening pass: authz on every route, upload size limits, path-traversal protection on `game_id`/`egg_id`, dependency audit (`cargo audit`).
- [ ] Basic observability: request logging, error tracking, and a startup self-check.

**Exit criteria (MVP done):** Single-user Nest runs in a container on Pikapods; a Windows Bird scans verified games, completes the play→exit→upload cycle, and restores saves — matching SPECS §6.

---

## Phase 12 — Long-Term Goals (Post-MVP)

**Goal:** Broaden platform reach and deployment convenience (SPECS §7).

- [ ] Full Ludusavi manifest coverage beyond the verified subset.
- [ ] Linux/SteamOS Bird support (Feather Agent + packaging for Steam Deck).
- [ ] macOS Bird support.
- [ ] Explore mobile (iOS/Android) companion/monitoring.
- [ ] One-click / templated Pikapods deployment and support for other container platforms.
- [ ] Multi-user hardening and quotas if demand grows.
- [ ] Optional: end-to-end encryption of Eggs, delta/incremental save uploads, and bandwidth optimization.

**Exit criteria:** The Bird runs on Windows, Linux/SteamOS, and macOS, and the Nest deploys to Pikapods (and at least one other platform) with minimal friction.

---

## Cross-Cutting Concerns (apply to every phase)

- [ ] Tests: unit + integration per feature; keep CI green on Linux + Windows.
- [ ] Error handling: no silent failures; user-facing conflicts always surfaced.
- [ ] Security: authz on every route, hashed passwords, path-traversal protection, input validation, dependency audits.
- [ ] Performance: keep the Nest lean (< 30 MB RAM) and the Bird light on CPU/battery.
- [ ] Docs: keep `README`, `SPECS.md`, and this `TODO.md` in sync as scope evolves.
