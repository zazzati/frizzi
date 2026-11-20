-- votes has PRIMARY KEY (participant_id, slot_id), which cannot serve lookups by slot alone:
-- the ON DELETE CASCADE from event_slots, tie-break counts and timezone resets all filter on slot_id.
CREATE INDEX IF NOT EXISTS idx_votes_slot ON votes(slot_id);

-- Both columns already carry UNIQUE constraints whose indexes are identical to these.
DROP INDEX IF EXISTS idx_events_public_token;
DROP INDEX IF EXISTS idx_participants_edit_token;

-- The mail claim filters pending rows and takes the oldest by created_at; inspect_queue reads
-- min(created_at) over the same rows.
CREATE INDEX IF NOT EXISTS idx_outbox_claim ON email_outbox (created_at) WHERE sent_at IS NULL;
DROP INDEX IF EXISTS idx_outbox_pending;

-- events(confirmed_slot_id) and events(tiebreak_slot_id) need no index: both foreign keys are
-- composite (slot, events.id), so their ON DELETE SET NULL actions are resolved via events_pkey.
