# Frizzi

Frizzi is a passwordless app for organising events and finding a date that works for your guests. The organiser creates a poll and receives a private management link by email. Guests can see the group’s availability, vote and receive their own link to edit their response. The organiser can update the details, confirm a date, cancel the event or reopen voting. Frizzi sends localised notifications when details change.

The frontend uses SvelteKit, TypeScript, Tailwind CSS and Lucide. The API uses Rust and Axum, with PostgreSQL for storage and Mailpit for development email. The public API keeps email addresses and private tokens out of its responses.

[Open Frizzi](https://frizzi.bid/)

## Home page

<p>
  <a href="docs/screenshots/home-desktop.png"><img src="docs/screenshots/home-desktop.png" alt="Frizzi’s English home page on desktop, showing the event creation form" width="700"></a>
</p>

<p>
  <a href="docs/screenshots/home-mobile.png"><img src="docs/screenshots/home-mobile.png" alt="Frizzi’s English home page on mobile, with the form arranged in a single column" width="300"></a>
</p>

## Project structure

| Path | Contents |
| --- | --- |
| `src/routes/` and `src/lib/` | Pages, components and frontend logic |
| `src/*.rs` and `src/mail/` | API, email, telemetry and maintenance |
| `api/index.rs` | Vercel entry point for the API |
| `migrations/` | PostgreSQL migrations |
| `locales/` | Interface and email translations |
| `tests/` | Integration and browser tests |

## Run locally with Docker

You need Node.js 22.12+ (or 24+), npm, stable Rust and Docker Compose.

```sh
test -f .env || install -m 600 .env.example .env
npm ci
docker compose up -d --wait
cargo run --bin migrate
npm run dev:full
```

Open [Frizzi](http://localhost:5173) and [Mailpit](http://localhost:8025). The Rust API listens on `127.0.0.1:3001`, and Vite proxies requests under `/api`. The migration command applies the SQLx migrations directly. If you already have `sqlx-cli`, you can also run `sqlx migrate run`.

`npm run dev:full` starts both the API and the frontend. To run them separately, use `cargo run --bin local` and `npm run dev`. The frontend reloads automatically. Restart the API after Rust changes or use `cargo watch -x 'run --bin local'`.

## Run locally without Docker

On Linux x86-64, you can run PostgreSQL 16 and Mailpit as user processes without administrator privileges.

```sh
test -f .env || install -m 600 .env.example .env
npm ci
npm run services:native
```

The script uses `embedded-postgres` and the official Mailpit release. PostgreSQL data persists in `~/.cache/frizzi/postgres-16`.

Set these values in `.env` and keep `APP_URL` and `MAIL_FROM` as shown in the example.

```dotenv
DATABASE_URL=postgres://frizzi:frizzi@localhost:55432/frizzi
SMTP_HOST=localhost
SMTP_PORT=11025
```

In a second terminal, run `cargo run --bin migrate` followed by `npm run dev:full`. [Mailpit](http://localhost:18025) runs on port 18025 in this setup.

## Languages

Frizzi supports Italian, English, Spanish, Simplified Chinese, Hindi, Arabic, French, Bengali, Portuguese, Russian and German. Italian is the default. The language selector saves the choice in the browser and switches to a right-to-left layout for Arabic. Frizzi also saves each organiser’s and guest’s chosen language for email notifications.

The JSON dictionaries in `locales/` are shared by `svelte-i18n` and `rust-i18n`. Dates and times follow the chosen locale in the browser and in emails. User-provided content stays in its original language. The translations have yet to receive a professional review.

## Location autocomplete

Set `GEOAPIFY_API_KEY` in your local `.env` or in your hosting provider’s server-side environment variables. The backend limits autocomplete to 120 searches per hour across the database. Consider this limit alongside any other projects that share the provider quota. Organisers can enter addresses, venue names and links manually when autocomplete is unavailable.

## Email delivery

Frizzi saves email jobs in the same database transaction as the operation that triggered them. Delivery starts after the transaction commits. If SMTP or Resend fails, the job stays in the queue and Frizzi retries with increasing delays. The local worker checks the queue every ten seconds. On Vercel, the included cron is scheduled for 06:00 UTC daily. More frequent retries require a scheduler supported by your hosting plan.

`GET /api/cron/email` requires `Authorization: Bearer <CRON_SECRET>` and processes up to 25 messages per call. Resend receives an idempotency key to reduce duplicates. SMTP provides at-least-once delivery, so an interruption between sending and recording success may result in a duplicate. Frizzi removes recipient addresses and links from the queue after successful submission to the provider.

Changing a proposed date or time clears the votes for that option and reopens the poll. Changing the time zone clears all votes. Unchanged options keep their votes. Switching to single-choice voting requires updating any existing responses that selected multiple dates.

## Environment variables

[`.env.example`](.env.example) lists the server-side environment variables and safe development defaults. Keep real credentials in a local `.env` file with permissions `0600` or in your hosting provider’s secret store. The Docker database and test mailbox bind only to loopback.

To enable PDF invitations, set `FRIZZI_TYPST` and `FRIZZI_PDF_FONTS`. The [Typst setup script](deploy/setup-invitation-pdf.py) installs the renderer and fonts on Linux x86-64. Frizzi sends the HTML and plain-text confirmation when Typst is unavailable.

## Checks

```sh
npm run test:setup
npm run check
npm run test:unit
npm run build
cargo test --lib
npm test
npm audit
```

If Chromium’s dependencies are missing on Linux, install them with `npx playwright install-deps chromium`. The suite also uses any browser libraries already installed in `~/.cache/frizzi/browser-libs`.

`npm run test:unit` runs the Vitest unit tests for the pure frontend logic in `src/lib` (date entry, poll results, error messages) and checks that every locale has the same keys. It needs no services. `npm run coverage` runs the same tests and writes a coverage report to `coverage/`.

`npm test` starts a temporary PostgreSQL 16 database, Mailpit, the Rust API and Vite on dedicated ports. Integration tests cover event creation, privacy, authorisation, isolation between events, voting, edits, localised notifications and SMTP retries. Browser checks cover all eleven languages, right-to-left layout, language persistence, the event workflow and mobile rendering. Screenshots go to `test-results/`. Test emails stay in the local Mailpit inbox.

## Retention

Polls remain available through the end of the tenth day after the event date, in the event’s time zone. If the organiser has not confirmed a date, the latest proposed date applies. Maintenance deletes expired event data and rotates aggregate usage metrics. The API emits structured JSON logs. Set `FRIZZI_LOG_DIR` to write the logs to daily files with local rotation.
