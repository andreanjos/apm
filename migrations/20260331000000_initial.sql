-- Initial schema for apm-server
-- Creates the foundation tables needed for the commerce layer.

CREATE TABLE IF NOT EXISTS schema_info (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO schema_info (key, value)
VALUES ('version', '1')
ON CONFLICT DO NOTHING;
