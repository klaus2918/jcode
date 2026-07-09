# jcode phone server (managed cloud host)

A self-managing EC2 host that runs `jcode serve` with the WebSocket gateway so
phones (the iOS app, or SSH clients like Termius) can drive jcode sessions
without any laptop in the loop. Billing safety is layered and each layer has
been live-tested.

Live deployment (July 2026): AWS account `302154194530`, us-east-1,
instance `i-08214cf66cd3f80c7` (m7i-flex.large), Elastic IP `54.196.207.97`.

## Architecture

```
phone (jcode iOS app / Termius)
  │  WebSocket :7643 (pair token auth)
  ▼
EC2 jcode server ──instance role──▶ AWS Bedrock (Opus 4.6 default)
  ▲
  │ tap wake link (API Gateway → wake lambda, token-protected)
  │ "Pair this phone" button (lambda → pair service :7644 → `jcode pair`)
  │
CloudWatch alarms ──▶ SNS jcode-guard-stop ──▶ breaker lambda ──▶ stop instance
                              └──▶ email
```

## Files

| File | Deployed at | Purpose |
|---|---|---|
| `units/jcode-serve.service` | `~ec2-user/.config/systemd/user/` (user unit, linger on) | jcode daemon + gateway, restart always |
| `units/jcode-pair.service` | `/etc/systemd/system/` | pairing-code HTTP service on :7644 |
| `units/idle-autostop.{service,timer}` | `/etc/systemd/system/` | 5-min check, poweroff after 30 min idle |
| `idle-autostop.sh` | `/usr/local/bin/` | idle = no gateway/SSH clients AND jcode has no outbound :443 (not streaming) |
| `jcode-pair-service.py` | `/usr/local/bin/` | token-protected `GET /pair-code` → runs `jcode pair`, returns code + `jcode://` deep link. Token in `/etc/jcode-pair-token` |
| `wake-lambda.py` | Lambda `jcode-phone-wake` (behind API Gateway `8c3wp4cbag`) | wake page: starts instance, polls health, pair button |
| `breaker-lambda.py` | Lambda `jcode-guard-breaker` | stops the instance; subscribed to SNS `jcode-guard-stop` |

## Server config (instance)

- `~/.jcode/config.toml`: `[provider]` default bedrock/Opus 4.6, `[gateway] enabled = true, port 7643, bind 0.0.0.0`
  (note: `~/.jcode/config.toml`, NOT `~/.config/jcode/`)
- `~/.bashrc` env: `JCODE_BEDROCK_ENABLE=1`, `AWS_REGION=us-east-1`,
  `JCODE_BEDROCK_MODEL=us.anthropic.claude-opus-4-6-v1`, `JCODE_GATEWAY_HOST=<elastic ip>`
- Helpers: `~/bin/jc` (jcode with bedrock), `~/bin/phone` (attach-or-create tmux jcode)
- `loginctl enable-linger ec2-user` so the user service runs at boot
- Instance attr `instance-initiated-shutdown-behavior=stop` so `poweroff` = stopped (not billed)

## Cost guardrails (all live-tested)

| Layer | Trigger | Action |
|---|---|---|
| idle-autostop | 30 min no clients + not streaming | instance powers itself off |
| `jcode-bedrock-tokens-warn` | >3M input tokens / 15 min | email |
| `jcode-bedrock-tokens-stop` | >10M input tokens / 15 min, 2 periods | breaker stops instance + email |
| `jcode-billing-warn-25` / `-stop-75` | EstimatedCharges > $25 / $75 | email / breaker + email |
| AWS budget `jcode-dev-monthly-cost` | $10/mo | email |

Stopped instance cost ≈ $6/mo (EBS 30GB + idle Elastic IP).

## Phone flow

1. Bookmark the wake link (`https://<api-id>.execute-api.us-east-1.amazonaws.com/?t=<token>`,
   token stored at `~/.jcode/jcode-phone-wake-token` on the workstation).
2. Tap it: instance starts, page polls every 5 s, flips to "Ready" (~15 s from stopped).
3. Tap "Pair this phone" → 6-digit code + `jcode://pair?...` deep link → opens the iOS app paired.
4. SSH fallback: Termius to the Elastic IP as `ec2-user`, run `phone`.

## Security notes

- Gateway `/pair` requires a live 6-digit code (5-min TTL); WS requires the
  bearer token minted at pairing. Tokens stored hashed server-side.
- Wake/pair lambda endpoints require the query token; wrong token = 403.
- SSH is key-only. Ports open: 22, 7643, 7644.
- **Tailscale**: the instance is on the tailnet as `jcode-phone`
  (100.109.78.41, `tailscale set --hostname jcode-phone`, tailscale-ssh off).
  Phones running Tailscale can pair/connect via the tailnet IP instead of the
  public Elastic IP, which wraps the plain `ws://` protocol in WireGuard.
  Hardening option (not enabled): remove 7643/7644 from the security group to
  go tailnet-only; this breaks the wake page's pair button (Lambda is not on
  the tailnet) but the wake/stop lifecycle still works via EC2 APIs.
- IAM: instance role has `AmazonBedrockFullAccess` only. Waker lambda:
  start/describe EC2 + logs. Breaker lambda: stop/describe EC2 + SNS publish.

## Rebuild from scratch (≈15 min)

1. Launch AL2023 x86_64, free-tier-eligible type, 30GB gp3, key pair, SG with 22/7643/7644.
2. Create role with `AmazonBedrockFullAccess`, instance profile, attach; associate Elastic IP.
3. Install jcode (`curl -fsSL .../install.sh | bash`), write config + env as above, `dnf install tmux git`.
4. Copy `units/*`, `idle-autostop.sh`, `jcode-pair-service.py` to the paths in the table; write `/etc/jcode-pair-token`; `systemctl enable --now` each; enable linger.
5. Deploy the two lambdas (`wake-lambda.py` with `TOKEN`/`INSTANCE_ID`/`HOST` updated, breaker with `INSTANCE_ID`), API Gateway HTTP API → wake lambda (function URLs are blocked by an account-level public-access block; use API Gateway).
6. Create the SNS topics/subscriptions and CloudWatch alarms (names above).
7. Test: breaker invoke stops the box; wake link starts it; pair button returns a working code (`ios/TestHarness/protocol_smoke_test.py --host <ip>`).
