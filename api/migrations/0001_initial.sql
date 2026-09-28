PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS credentials (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    reservation_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('issued', 'revoked', 'failed')),
    provider TEXT NOT NULL CHECK (provider = 'mock'),
    issued_at TEXT NOT NULL,
    body_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS credentials_by_owner
    ON credentials(owner_id, issued_at DESC);

CREATE TABLE IF NOT EXISTS reservations (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    gate_id TEXT NOT NULL,
    starts_at TEXT NOT NULL,
    ends_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'cancelled')),
    created_at TEXT NOT NULL,
    session_id TEXT NOT NULL,
    body_json TEXT NOT NULL,
    CHECK (starts_at < ends_at)
);

CREATE INDEX IF NOT EXISTS reservations_by_gate_time
    ON reservations(gate_id, status, starts_at, ends_at);
CREATE INDEX IF NOT EXISTS reservations_by_owner
    ON reservations(owner_id, created_at DESC);

CREATE TRIGGER IF NOT EXISTS reservations_prevent_overlap_insert
BEFORE INSERT ON reservations
WHEN NEW.status = 'active'
 AND EXISTS (
    SELECT 1 FROM reservations existing
    WHERE existing.gate_id = NEW.gate_id
      AND existing.status = 'active'
      AND existing.starts_at < NEW.ends_at
      AND existing.ends_at > NEW.starts_at
 )
BEGIN
    SELECT RAISE(ABORT, 'reservation_overlap');
END;

CREATE TRIGGER IF NOT EXISTS reservations_prevent_overlap_update
BEFORE UPDATE OF gate_id, starts_at, ends_at, status ON reservations
WHEN NEW.status = 'active'
 AND EXISTS (
    SELECT 1 FROM reservations existing
    WHERE existing.id <> NEW.id
      AND existing.gate_id = NEW.gate_id
      AND existing.status = 'active'
      AND existing.starts_at < NEW.ends_at
      AND existing.ends_at > NEW.starts_at
 )
BEGIN
    SELECT RAISE(ABORT, 'reservation_overlap');
END;

CREATE TABLE IF NOT EXISTS registrations (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    credential_id TEXT NOT NULL REFERENCES credentials(id),
    reservation_id TEXT NOT NULL REFERENCES reservations(id),
    gate_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('registration_pending', 'registered', 'revocation_pending', 'revoked', 'failed')
    ),
    adapter_mode TEXT NOT NULL CHECK (adapter_mode = 'mock'),
    gate_applied INTEGER NOT NULL DEFAULT 0 CHECK (gate_applied = 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    session_id TEXT NOT NULL,
    body_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS registrations_by_owner
    ON registrations(owner_id, created_at DESC);
CREATE INDEX IF NOT EXISTS registrations_by_reservation
    ON registrations(owner_id, reservation_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS registrations_prevent_duplicates
    ON registrations(owner_id, credential_id, reservation_id, gate_id)
    WHERE status IN ('registration_pending', 'registered', 'failed');

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    action TEXT NOT NULL,
    result TEXT NOT NULL,
    detail TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode = 'mock'),
    occurred_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS audit_events_by_session
    ON audit_events(owner_id, session_id, occurred_at);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    owner_id TEXT NOT NULL,
    route TEXT NOT NULL,
    request_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    response_status INTEGER NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (owner_id, route, request_id)
);
