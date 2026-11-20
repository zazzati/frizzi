use sqlx::PgPool;

pub async fn purge(pool: &PgPool) -> Result<(u64, u64), sqlx::Error> {
    // A concurrent reschedule updates updated_at, so a stale candidate cannot be deleted.
    // A zone name that this server's tz database lacks would make timezone() raise and abort
    // the whole purge, so such rows fall back to UTC instead.
    let events = sqlx::query(
        "WITH zones AS MATERIALIZED (SELECT name FROM pg_timezone_names),
        expired AS MATERIALIZED (
            SELECT e.id, e.updated_at
            FROM events e
            LEFT JOIN zones z ON z.name = e.timezone
            CROSS JOIN LATERAL (
                SELECT slot_date + 11 AS expiry
                FROM event_slots
                WHERE event_id = e.id AND (e.confirmed_slot_id IS NULL OR id = e.confirmed_slot_id)
                ORDER BY slot_date DESC LIMIT 1
            ) due
            WHERE due.expiry <= timezone(COALESCE(z.name, 'UTC'), now())::date
        )
        DELETE FROM events e USING expired
        WHERE e.id = expired.id AND e.updated_at = expired.updated_at",
    )
    .execute(pool)
    .await?;

    // Pending legacy messages without a recoverable event link also have a retention limit.
    let legacy_mail = sqlx::query(
        "DELETE FROM email_outbox
         WHERE event_id IS NULL AND sent_at IS NULL AND created_at < now() - interval '10 days'",
    )
    .execute(pool)
    .await?;
    Ok((events.rows_affected(), legacy_mail.rows_affected()))
}
