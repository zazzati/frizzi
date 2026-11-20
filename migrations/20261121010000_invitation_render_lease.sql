-- Short claim on a pending PDF render so Typst can run without holding a row lock or a pooled
-- connection; an expired lease (crashed or frozen worker) lets the next dispatcher render.
ALTER TABLE confirmation_invitations ADD COLUMN IF NOT EXISTS render_locked_until TIMESTAMPTZ;
