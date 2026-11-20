-- Messages that the provider rejected or that exhausted their retries stop being claimed.
ALTER TABLE email_outbox ADD COLUMN IF NOT EXISTS failed_at TIMESTAMPTZ;

-- The claim (and inspect_queue) only look at rows that are neither sent nor failed.
DROP INDEX IF EXISTS idx_outbox_pending;
DROP INDEX IF EXISTS idx_outbox_claim;
CREATE INDEX idx_outbox_claim ON email_outbox (created_at) WHERE sent_at IS NULL AND failed_at IS NULL;
