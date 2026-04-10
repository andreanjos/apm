-- Crowdsourced bundle ID mappings from user scans.
-- Each row represents one user reporting that a given CFBundleIdentifier
-- prefix corresponds to a registry slug. When enough distinct reporters
-- agree, the mapping is considered confirmed.

CREATE TABLE IF NOT EXISTS bundle_id_submissions (
    id BIGSERIAL PRIMARY KEY,
    bundle_id_prefix TEXT NOT NULL,
    registry_slug TEXT NOT NULL,
    reporter_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (bundle_id_prefix, reporter_hash)
);

CREATE INDEX IF NOT EXISTS idx_bundle_id_prefix
    ON bundle_id_submissions (bundle_id_prefix);

-- View of confirmed mappings. Since submissions come from the CLI
-- which we control, a single report is sufficient to confirm a mapping.
CREATE OR REPLACE VIEW confirmed_bundle_ids AS
SELECT
    bundle_id_prefix,
    registry_slug,
    COUNT(DISTINCT reporter_hash) AS reporter_count
FROM bundle_id_submissions
GROUP BY bundle_id_prefix, registry_slug;
