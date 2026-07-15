-- Sync-state and conflict tracking for Phase 5.
-- Timestamps are stored as INTEGER Unix epoch seconds (UTC).

-- Tracks the last Egg a Bird knew it was in sync with for each Clutch.
-- This baseline is what lets the server detect divergence and "Chilly Egg"
-- conflicts when the Bird reports its local hash + modified time.
CREATE TABLE IF NOT EXISTS bird_clutch_sync (
    bird_id          TEXT    NOT NULL,
    clutch_id        TEXT    NOT NULL,
    last_egg_id      TEXT,
    last_synced_hash TEXT,
    last_synced_at   INTEGER,
    status           TEXT    NOT NULL DEFAULT ('safe_in_nest'),
    created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),

    PRIMARY KEY (bird_id, clutch_id),

    FOREIGN KEY (bird_id)   REFERENCES birds(id)   ON DELETE CASCADE,
    FOREIGN KEY (clutch_id) REFERENCES clutches(id) ON DELETE CASCADE,
    FOREIGN KEY (last_egg_id) REFERENCES eggs(id)  ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_bird_clutch_sync_clutch_id
    ON bird_clutch_sync (clutch_id);

CREATE INDEX IF NOT EXISTS idx_bird_clutch_sync_bird_id
    ON bird_clutch_sync (bird_id);
