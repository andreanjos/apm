-- Agent purchase policies and spending limits for Phase 10

CREATE TABLE IF NOT EXISTS api_key_purchase_policies (
    api_key_id BIGINT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
    preauthorized_payment_method BOOLEAN NOT NULL DEFAULT FALSE,
    per_transaction_limit_cents BIGINT,
    period_limit_cents BIGINT,
    period_spent_cents BIGINT NOT NULL DEFAULT 0,
    period_started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS agent_purchase_attempts (
    id BIGSERIAL PRIMARY KEY,
    api_key_id BIGINT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plugin_slug TEXT NOT NULL,
    amount_cents BIGINT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'usd',
    outcome TEXT NOT NULL,
    denial_code TEXT,
    order_id BIGINT REFERENCES orders(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
