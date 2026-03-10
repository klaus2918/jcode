# jcode Telemetry

jcode collects **anonymous, minimal usage statistics** to help understand how many people use jcode and what providers/models are popular. This data helps prioritize development.

## What We Collect

### Install Event (sent once, on first launch)

| Field | Example | Purpose |
|-------|---------|----------|
| `id` | `a1b2c3d4-...` | Random UUID, not tied to your identity |
| `event` | `"install"` | Event type |
| `version` | `"0.6.0"` | jcode version |
| `os` | `"linux"` | Operating system |
| `arch` | `"x86_64"` | CPU architecture |

### Session End Event (sent when you close jcode)

| Field | Example | Purpose |
|-------|---------|----------|
| `id` | `a1b2c3d4-...` | Same random UUID |
| `event` | `"session_end"` | Event type |
| `version` | `"0.6.0"` | jcode version |
| `os` | `"linux"` | Operating system |
| `arch` | `"x86_64"` | CPU architecture |
| `provider_start` | `"claude"` | Provider when session started |
| `provider_end` | `"claude"` | Provider when session ended |
| `model_start` | `"claude-sonnet-4-20250514"` | Model when session started |
| `model_end` | `"claude-sonnet-4-20250514"` | Model when session ended |
| `provider_switches` | `0` | How many times you switched providers |
| `model_switches` | `1` | How many times you switched models |
| `duration_mins` | `45` | Session length in minutes |
| `turns` | `23` | Number of messages you sent |
| `errors` | `{"provider_timeout": 0, ...}` | Count of errors by category |

## What We Do NOT Collect

- No file paths, project names, or directory structures
- No code, prompts, or LLM responses
- No tool names or tool outputs
- No MCP server names or configurations
- No IP addresses (Cloudflare Workers don't log these by default)
- No personal information of any kind
- No error messages (only category counts like "2 timeouts")

The UUID is randomly generated on first run and stored at `~/.jcode/telemetry_id`. It is not derived from your machine, username, email, or any identifiable information.

## How It Works

1. On first launch, jcode generates a random UUID and sends an `install` event
2. When you close a session, jcode sends a `session_end` event with session metrics
3. Both requests are fire-and-forget HTTP POSTs that don't block startup or shutdown
4. If the request fails (offline, firewall, etc.), jcode silently continues - no retries, no queuing

The telemetry endpoint is a Cloudflare Worker that stores events in a D1 database. The source code for the worker is in [`telemetry-worker/`](./telemetry-worker/).

## How to Opt Out

Any of these methods will disable telemetry completely:

```bash
# Option 1: Environment variable
export JCODE_NO_TELEMETRY=1

# Option 2: Standard DO_NOT_TRACK (https://consoledonottrack.com/)
export DO_NOT_TRACK=1

# Option 3: File-based opt-out
touch ~/.jcode/no_telemetry
```

When opted out, zero network requests are made. The telemetry module short-circuits immediately.

## Verification

This is open source. The entire telemetry implementation is in [`src/telemetry.rs`](./src/telemetry.rs) - you can read exactly what gets sent. There are no other network calls related to telemetry anywhere in the codebase.

## Data Retention

Telemetry data is used in aggregate only (total installs, active users per week, provider distribution). Individual event records are retained for up to 12 months and then deleted.
