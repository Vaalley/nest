-- Initial schema for the Nest server.
-- Timestamps are stored as INTEGER Unix epoch seconds (UTC).

CREATE TABLE IF NOT EXISTS flocks (
    id            TEXT    PRIMARY KEY NOT NULL,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS birds (
    id         TEXT    PRIMARY KEY NOT NULL,
    flock_id   TEXT    NOT NULL REFERENCES flocks (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    platform   TEXT    NOT NULL,
    last_seen  INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_birds_flock_id ON birds (flock_id);

CREATE TABLE IF NOT EXISTS clutches (
    id          TEXT    PRIMARY KEY NOT NULL,
    flock_id    TEXT    NOT NULL REFERENCES flocks (id) ON DELETE CASCADE,
    game_id     TEXT    NOT NULL,
    brood_limit INTEGER NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (flock_id, game_id)
);

CREATE INDEX IF NOT EXISTS idx_clutches_flock_id ON clutches (flock_id);

CREATE TABLE IF NOT EXISTS eggs (
    id             TEXT    PRIMARY KEY NOT NULL,
    clutch_id      TEXT    NOT NULL REFERENCES clutches (id) ON DELETE CASCADE,
    source_bird_id TEXT    REFERENCES birds (id) ON DELETE SET NULL,
    file_hash      TEXT    NOT NULL,
    size_bytes     INTEGER NOT NULL,
    file_path      TEXT    NOT NULL,
    created_at     INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_eggs_clutch_id ON eggs (clutch_id);
