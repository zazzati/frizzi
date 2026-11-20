-- Transient PDF render failures are retried a bounded number of times before falling back.
ALTER TABLE confirmation_invitations ADD COLUMN IF NOT EXISTS render_attempts INTEGER NOT NULL DEFAULT 0;
