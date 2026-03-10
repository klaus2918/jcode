# jcode Telemetry Worker

Cloudflare Worker that receives anonymous telemetry events from jcode.

## Setup

1. Install wrangler: `npm install`

2. Create D1 database:
   ```bash
   wrangler d1 create jcode-telemetry
   ```

3. Update `wrangler.toml` with the database ID from step 2

4. Initialize schema:
   ```bash
   wrangler d1 execute jcode-telemetry --file=schema.sql
   ```

5. Deploy:
   ```bash
   npm run deploy
   ```

6. Set up custom domain (optional): point `telemetry.jcode.dev` to the worker in Cloudflare dashboard

## Querying Data

```bash
# Total installs
wrangler d1 execute jcode-telemetry --command "SELECT COUNT(DISTINCT telemetry_id) FROM events WHERE event = 'install'"

# Active users this week
wrangler d1 execute jcode-telemetry --command "SELECT COUNT(DISTINCT telemetry_id) FROM events WHERE event = 'session_end' AND created_at > datetime('now', '-7 days')"

# Provider distribution
wrangler d1 execute jcode-telemetry --command "SELECT provider_end, COUNT(*) as sessions FROM events WHERE event = 'session_end' GROUP BY provider_end ORDER BY sessions DESC"

# Average session duration
wrangler d1 execute jcode-telemetry --command "SELECT AVG(duration_mins) as avg_mins, AVG(turns) as avg_turns FROM events WHERE event = 'session_end'"

# Error rates
wrangler d1 execute jcode-telemetry --command "SELECT SUM(error_provider_timeout) as timeouts, SUM(error_rate_limited) as rate_limits, SUM(error_auth_failed) as auth_failures FROM events WHERE event = 'session_end'"

# Version adoption
wrangler d1 execute jcode-telemetry --command "SELECT version, COUNT(DISTINCT telemetry_id) as users FROM events GROUP BY version ORDER BY version DESC"

# OS/arch breakdown
wrangler d1 execute jcode-telemetry --command "SELECT os, arch, COUNT(DISTINCT telemetry_id) as users FROM events GROUP BY os, arch ORDER BY users DESC"
```
