<div align="center">

# jcode

[![Latest Release](https://badgen.net/github/release/1jehuang/jcode?icon=github)](https://github.com/1jehuang/jcode/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-blue?style=flat-square)](https://github.com/1jehuang/jcode/releases)
[![Last Commit](https://badgen.net/github/last-commit/1jehuang/jcode/master?icon=github)](https://github.com/1jehuang/jcode/commits/master)
[![GitHub Stars](https://badgen.net/github/stars/1jehuang/jcode?icon=github)](https://github.com/1jehuang/jcode/stargazers)
[![Discord](https://img.shields.io/badge/Discord-Join%20Community-5865F2?style=flat-square&logo=discord&logoColor=white)](https://discord.gg/nBe9vGyK9a)

The most RAM efficient harness <br>
The most most intelligent harness

<a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-memory-demo.mp4">
  <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-memory-demo.webp" alt="jcode memory demonstration" width="800">
</a>

<br>

[Website](https://jcode.sh) · [Docs](https://jcode.sh/docs) · [Benchmarks](https://jcode.sh/bench) · [Features](#features) · [Install](#installation) · [Quick Start](#quick-start) · [Further Reading](#further-reading) · [Contributing](CONTRIBUTING.md)

</div>

---

<div align="center">

## Installation

</div>

```bash
# macOS & Linux
curl -fsSL https://jcode.sh/install | bash
```

```powershell
# Windows 11 (PowerShell 5.1+)
irm https://jcode.sh/install.ps1 | iex
```

Need Homebrew, source builds, provider setup, or want an agent to set it up for you?
[Jump to detailed installation](#detailed-installation).

---


<div align="center">

## Performance & Resource Efficiency

</div>

jcode is built to be as performant and resource efficient as possible. Every metric is optimized to the bone, which is important for scaling multi-session workflows. Here we sample a few metrics to show the difference: RAM usage and boot up.

### RAM comparison

<div align="center">

<table>
  <tr>
    <td valign="top" align="center" width="50%">
      <strong>1 active session</strong>
      <table>
        <thead>
          <tr>
            <th>Tool</th>
            <th>PSS</th>
            <th>Comparison</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>jcode (local embedding off)</strong></td>
            <td align="right"><strong>27.8 MB</strong></td>
            <td align="right">baseline</td>
          </tr>
          <tr>
            <td><strong>jcode</strong></td>
            <td align="right"><strong>167.1 MB</strong></td>
            <td align="right"><strong>6.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>pi</strong></td>
            <td align="right"><strong>144.4 MB</strong></td>
            <td align="right"><strong>5.2× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Codex CLI</strong></td>
            <td align="right"><strong>140.0 MB</strong></td>
            <td align="right"><strong>5.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>OpenCode</strong></td>
            <td align="right"><strong>371.5 MB</strong></td>
            <td align="right"><strong>13.4× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>GitHub Copilot CLI</strong></td>
            <td align="right"><strong>333.3 MB</strong></td>
            <td align="right"><strong>12.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Cursor Agent</strong></td>
            <td align="right"><strong>214.9 MB</strong></td>
            <td align="right"><strong>7.7× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Claude Code</strong></td>
            <td align="right"><strong>386.6 MB</strong></td>
            <td align="right"><strong>13.9× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Antigravity CLI</strong></td>
            <td align="right"><strong>243.7 MB</strong></td>
            <td align="right"><strong>8.8× more RAM</strong></td>
          </tr>
        </tbody>
      </table>
    </td>
    <td width="24"></td>
    <td valign="top" align="center" width="50%">
      <strong>10 active sessions</strong>
      <table>
        <thead>
          <tr>
            <th>Tool</th>
            <th>PSS</th>
            <th>Comparison</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><strong>jcode (local embedding off)</strong></td>
            <td align="right"><strong>117.0 MB</strong></td>
            <td align="right">baseline</td>
          </tr>
          <tr>
            <td><strong>jcode</strong></td>
            <td align="right"><strong>260.8 MB</strong></td>
            <td align="right"><strong>2.2× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>pi</strong></td>
            <td align="right"><strong>833.0 MB</strong></td>
            <td align="right"><strong>7.1× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Codex CLI</strong></td>
            <td align="right"><strong>334.8 MB</strong></td>
            <td align="right"><strong>2.9× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>OpenCode</strong></td>
            <td align="right"><strong>3237.2 MB</strong></td>
            <td align="right"><strong>27.7× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>GitHub Copilot CLI</strong></td>
            <td align="right"><strong>1756.5 MB</strong></td>
            <td align="right"><strong>15.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Cursor Agent</strong></td>
            <td align="right"><strong>1632.4 MB</strong></td>
            <td align="right"><strong>14.0× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Claude Code</strong></td>
            <td align="right"><strong>2300.6 MB</strong></td>
            <td align="right"><strong>19.7× more RAM</strong></td>
          </tr>
          <tr>
            <td><strong>Antigravity CLI</strong></td>
            <td align="right"><strong>1021.2 MB</strong></td>
            <td align="right"><strong>8.7× more RAM</strong></td>
          </tr>
        </tbody>
      </table>
    </td>
  </tr>
</table>

</div>

### Time to first frame

<div align="center">

| Tool | Time to first frame | Range | Comparison |
|---|---:|---:|---:|
| **jcode** | **14.0 ms** | 10.1–19.3 ms | baseline |
| **Antigravity CLI** | **383.5 ms** | 363.1–415.4 ms | **27.4× slower** |
| **pi** | **590.7 ms** | 369.6–934.8 ms | **42.2× slower** |
| **Codex CLI** | **882.8 ms** | 742.3–1640.9 ms | **63.1× slower** |
| **OpenCode** | **1035.9 ms** | 922.5–1104.4 ms | **74.0× slower** |
| **GitHub Copilot CLI** | **1518.6 ms** | 1357.4–1826.8 ms | **108.5× slower** |
| **Cursor Agent** | **1949.7 ms** | 1711.0–2104.8 ms | **139.3× slower** |
| **Claude Code** | **3436.9 ms** | 2032.7–8927.2 ms | **245.5× slower** |

</div>

Measured on this Linux machine across 10 interactive PTY launches.

### Time to first input
(time until typed probe text appears on the rendered screen; Antigravity uses its internal input-ready log marker because the sign-in screen suppresses probe echo.)
<div align="center">

| Tool | Time to first input | Range | Comparison |
|---|---:|---:|---:|
| **jcode** | **48.7 ms** | 30.3–62.7 ms | baseline |
| **Antigravity CLI** | **383.7 ms** | 363.4–415.7 ms | **7.9× slower** |
| **pi** | **596.4 ms** | 373.9–955.2 ms | **12.2× slower** |
| **Codex CLI** | **905.8 ms** | 760.1–1675.7 ms | **18.6× slower** |
| **OpenCode** | **1047.9 ms** | 931.1–1116.9 ms | **21.5× slower** |
| **GitHub Copilot CLI** | **1583.4 ms** | 1422.8–1880.0 ms | **32.5× slower** |
| **Cursor Agent** | **1978.7 ms** | 1727.3–2130.0 ms | **40.6× slower** |
| **Claude Code** | **3512.8 ms** | 2137.4–9002.0 ms | **72.2× slower** |

</div>

Measured on this Linux machine across 10 interactive PTY launches. Antigravity CLI was unauthenticated for this run; its sign-in screen rendered normally and emitted an internal `CLI ready for user input` marker, but did not echo the typed probe.

### Additional clients / memory scaling

<div align="center">

| Tool | Extra PSS per added session | Comparison |
|---|---:|---:|
| **jcode (local embedding off)** | **~9.9 MB** | baseline |
| **jcode** | **~10.4 MB** | **1.1× more RAM** |
| **pi** | **~76.5 MB** | **7.7× more RAM** |
| **Codex CLI** | **~21.6 MB** | **2.2× more RAM** |
| **OpenCode** | **~318.4 MB** | **32.2× more RAM** |
| **GitHub Copilot CLI** | **~158.1 MB** | **16.0× more RAM** |
| **Cursor Agent** | **~157.5 MB** | **15.9× more RAM** |
| **Claude Code** | **~212.7 MB** | **21.5× more RAM** |
| **Antigravity CLI** | **~86.4 MB** | **8.7× more RAM** |

</div>
versions tested for this corrected memory rerun:

- `jcode v0.9.1888-dev (be386f2)`
- `pi 0.62.0`
- `codex-cli 0.120.0`
- `opencode 1.0.203`
- `GitHub Copilot CLI 1.0.24` for the 1-session rerun, `GitHub Copilot CLI 1.0.27` for the 10-session rerun
- `Cursor Agent 2026.04.08-a41fba1`
- `Claude Code 2.1.86 (Claude Code)`
- `Antigravity CLI 1.0.0`

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-performance-demo.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-performance-demo.webp" alt="jcode performance demonstration" width="900">
  </a>

  <p><em>jcode performance demonstration</em></p>

</div>


---

## Memory (Agent memory)

Jcode embeds each turn/response as a semantic vector. Every turn does queries a graph of memories to efficiently find related memory entries via a cosine similarity check. The embedding hits are fed into the conversation, or optionally uses a memory sideagent which verifies the memories are relevant, and potentially does more work for information retreival before injecting into the conversation. This results in a human like memory system which allows the agent to automatically recall relevant information to the conversation without actively calling memory tools or being a token burner. 
ot 
To have memories which are retrieved, they must also be extracted and stored. Every so often (semantic drift, K turns since last extraction, session end, etc), memories are extracted via a memory sideagent, and put into the memory graph. 

The harness also provides explicit memory tools to allow the agent to actively search or store the memory without relying on a passive background process. The harness also provides session search for traditional RAG on previous sessions. 

Memories are automatically consolidated every so often via the ambient mode. This reorganizes, checks for staleness and conflicts, etc

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-memory-demo.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-memory-demo.webp" alt="jcode memory demonstration" width="900">
  </a>

  <p><em>jcode memory demonstration</em></p>

</div>

<!-- Memory demo media is hosted in the readme-assets release. -->

---

## UI: Side panels, Diagrams, Info Widgets, rendering, scrolling, alignment

The side panel is a place for auxiliary information. Tell your jcode agent to load a file into the side panel and see it update in real time, or tell your agent to write directly to the side panel, or use it as a diff viewer. The side panel (and chat) is able to render mermaid diagrams inline. 
<img width="2877" height="1762" alt="image" src="https://github.com/user-attachments/assets/6c7bec81-ef3f-434d-8a7b-d55f8a54e5cf" />

To make this possible, I created a new mermaid rendering library to render diagrams 1800x faster. It has no browser or Typescript dependency. See https://github.com/1jehuang/mermaid-rs-renderer

To show you important information without taking space away from the screen that could be used for responses, I developed info widgets. Info widgets will only ever take up the negative space on the screen to show you information, and will get out of the way if there isn't any. 

Jcode can render at over a thousand fps. Your monitor will not have the refresh rate to show you, but this means you will not have silly flicker problems. 

The custom scrollback implementation of jcode allows it to do much more than a native scrollback. However, it is a terminal-level limitation that I cannot have smooth, partial line scrolling with a custom scrollback. To fix this, I made my own terminal. Handterm https://github.com/1jehuang/handterm implements a native scroll api, and also happens to be very efficient. This is a work in progress. Scrolling is still well implemented for normal terminals.

Jcode is left-aligned by default. You can switch to centered mode with the `Alt+C` hotkey, with the `/alignment` command, or in the config.

To disable emoji globally in TUI and CLI output, set `emoji = false` under `[display]` in `~/.jcode/config.toml`, or launch with `JCODE_NO_EMOJI=1`. Jcode replaces emoji with compact ASCII markers while preserving other Unicode text.

---

## Swarm

Spawn two or more agents in the same repo, and they will automatically be managed by the server to allow native collaboration. When agent A edits a file that agent B has read (code shifting under its feet), the server notifies agent B. Agent B can ignore it if it is not relevant, or it can check the diff to make sure that it doesn't conflict. Each agent has messaging abilities, capable of DMing just one agent, broadcasting to all other agents hosted by the server, or just agents working in that repo. This allows you to spawn multiple sessions in the same repo, and have all conflicts automatically resolved.

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/swarm-demo.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-swarm-demonstration.webp" alt="jcode swarm demonstration" width="900">
  </a>

  <p><em>jcode swarm demonstration</em></p>

</div>

Agents are also able to spawn their own swarms autonomously. They have a swarm tool which allows them to spawn in their own teamates to accomplish tasks in parallel. Doing so turns the main agent into a coordinator and the spawned agents into workers. Groups of agents, their messaging channels, their completion statuses, etc are all automatically managed. This can be done headlessly or headed.

---

## OAuth and Providers

jcode works with subscription-backed OAuth flows and many provider integrations, so you can use the models you already pay for and still fall back to direct API providers when needed.

### Model access is configuration-driven

jcode connects models exclusively through `[[providers]]` config entries and a
unified credential file, resonix-style. There is no interactive login command:
you configure an endpoint, store its key, and pick it with `/model`.

- **Add a provider:** `jcode provider add <name> --base-url <url> --model <id> --api-key-env <ENV_VAR>`
- **Store the key:** the value goes in the unified `~/.jcode/.env` file (`ENV_VAR=value`), never in `config.toml`
- **Use it:** `jcode --provider <name> run '...'`, or pick it interactively with `/model`

Local endpoints (Ollama, LM Studio) need no key: `jcode provider add ollama-local --base-url http://localhost:11434/v1 --model llama3.2 --no-api-key`.

#### Local gateway (CC Switch proxy)

jcode can also connect through a [CC Switch](https://github.com/farion1231/cc-switch) local proxy: CC Switch holds your real API keys, runs a local proxy (default `http://127.0.0.1:15721`, shown in its proxy panel), and forwards requests to whichever provider you have enabled there, with failover and format conversion handled by the proxy. Point jcode at the proxy as an Anthropic-format endpoint:

```toml
[provider]
default_provider = "cc-switch"
# default_model is optional here: leave it unset to auto-follow the provider
# you enable in CC Switch (see the two rules below), or set it to pin a model.

# The canonical on-disk style is named tables: [providers.<name>] plus
# [[providers.<name>.models]] (resonix-style top-level [[providers]] arrays
# are accepted for migration but never written by jcode).
[providers.cc-switch]
type = "openai-compatible"
base_url = "http://127.0.0.1:15721"   # address shown in the CC Switch proxy panel
api = "anthropic"                     # proxy routes this as a Claude-channel request
auth = "none"                         # CC Switch injects the real key
replay_reasoning_content = true       # DeepSeek 风格网关要求回传无签名 thinking

[[providers.cc-switch.models]]
id = "deepseek-v4-flash"              # optional; leave unset to auto-follow the provider
context_window = 1000000
auth = "none"
```

Rules to remember:

- **The model is optional.** Leave `default_model` (and the `models` list) unset and jcode fetches the proxy's model catalog (`/v1/models`) at startup, automatically picking the first model of whichever provider is currently enabled in CC Switch. Switch providers inside CC Switch and jcode follows: when a request fails because the model is unavailable, it re-fetches the catalog and retries with a catalog model. To pin a specific model, set `default_model` (or pass `--model <id>`); an explicit choice is never overridden by auto-discovery.
- **The key is not required.** Set `auth = "none"`; CC Switch injects the real key at the proxy.
- **DeepSeek-style gateways must echo thinking.** Some Anthropic-format gateways (e.g. proxies fronting DeepSeek) return thinking without an Anthropic signature and reject follow-up requests whose assistant history omits it (`reasoning_content must be passed back`). Set `replay_reasoning_content = true` on the profile to echo the unsigned thinking back on later turns. The official Anthropic API instead rejects unsigned thinking blocks, so keep it off there (the default).

See [docs/Provider迁移指南.md](docs/Provider迁移指南.md) for the full guide.

### Config-file setup for self-hosted endpoints and MCP

If you prefer to configure things by editing files instead of using the login UI, jcode supports both a custom OpenAI-compatible endpoint config and MCP config files.

#### OpenAI-compatible providers

Many hosted services speak the standard OpenAI `/v1/chat/completions` API. jcode talks to them through one shared OpenAI-compatible provider, so you can use almost any such endpoint without waiting for a dedicated integration.

There is one way to set one up:

- **`jcode provider add`** — the one-shot profile command writes a named profile and stores the key in the unified `~/.jcode/.env`:

  ```bash
  jcode provider add openrouter --base-url https://openrouter.ai/api/v1 --model deepseek/deepseek-chat --api-key-env OPENROUTER_API_KEY
  jcode provider add ollama-local --base-url http://localhost:11434/v1 --model llama3.2 --no-api-key
  jcode provider add my-gateway --base-url https://gateway.example.com/v1 --model gpt-5.5 --api-key-stdin
  ```

  jcode never hardcodes vendor names — every service is a `[providers.<name>]` config entry (or the equivalent resonix-style `[[providers]]` array entry). The key lives in `~/.jcode/.env` under the `api_key_env` variable name; pick any configured provider/model with `/model`.

Useful environment overrides for these endpoints:

- `JCODE_STREAM_IDLE_TIMEOUT_SECS` — raise the base streaming idle timeout (default 180s) for slow reasoning models that think silently before emitting tokens. High reasoning efforts scale this automatically (high 2x, xhigh 3x, max 4x). Also settable as `[provider] stream_idle_timeout_secs` in `config.toml`.
- Per-model `context_window` (alias `context_limit`) in a `[[providers.<name>.models]]` entry — set the context window when the endpoint has no usable `/v1/models` response, so jcode does not fall back to the generic 200k default.
- `extra_body` — inject non-standard top-level fields into every chat/completions request body for backends that require them. See [Extra request-body fields](#extra-request-body-fields-extra_body) below.

For details on self-hosting, local runtimes, and the exact config file shape, see below.

#### Self-hosted OpenAI-compatible endpoints, including vLLM

For agents and scripts, the preferred path is the one-shot provider profile command. It writes a named profile to `~/.jcode/config.toml`, stores secrets in jcode's private app config directory when requested, and prints exact run/validation commands:

```bash
# Secret-safe setup for a hosted OpenAI-compatible API.
printf '%s' "$MY_API_KEY" | jcode provider add my-api \
  --base-url https://llm.example.com/v1 \
  --model my-model-id \
  --api-key-stdin \
  --set-default \
  --json

# Smoke test the profile.
jcode --provider-profile my-api auth-test --prompt 'Reply exactly JCODE_PROVIDER_SETUP_OK'

# Use it directly.
jcode --provider-profile my-api run 'hello'
```

For local servers that do not require auth:

```bash
jcode provider add local-vllm \
  --base-url http://localhost:8000/v1 \
  --model Qwen/Qwen3-Coder-30B-A3B-Instruct \
  --no-api-key \
  --set-default
```

Built-in local profiles are available for the common desktop/local runtimes:

```bash
# Ollama: start the local server and install a model first.
ollama pull llama3.2
jcode provider add ollama-local --base-url http://localhost:11434/v1 --model llama3.2 --no-api-key
jcode --provider ollama-local run 'hello'

# LM Studio: start the Local Server, load a chat model, then use the exact
# model identifier shown by LM Studio or by curl http://localhost:1234/v1/models.
jcode provider add lmstudio --base-url http://localhost:1234/v1 --model '<model-id>' --no-api-key
jcode --provider lmstudio run 'hello'
```

Ollama and LM Studio both expose OpenAI-compatible `/v1/models` and `/v1/chat/completions` endpoints. jcode uses streaming chat completions, function/tool calling, and OpenAI-style image content for vision-capable local models. If a local server requires a token, use `--api-key-stdin` and store it in the unified `~/.jcode/.env`.

Useful flags:

- `--api-key-env NAME`: reference an existing environment variable instead of storing a key.
- `--api-key-stdin`: read and store a key without putting it in shell history.
- `--context-window TOKENS`: persist the model context window for model selection and routing.
- `--overwrite`: replace an existing profile of the same name.
- `--model-catalog`: use the endpoint's `/models` response in addition to configured models.

The generated profile can also be edited manually in `~/.jcode/config.toml`. Two equivalent styles are accepted — the concise resonix-style top-level `[[providers]]` array (recommended) and the classic `[providers.<name>]` table:

```toml
[provider]
default_provider = "my-api"
default_model = "my-model-id"

# Resonix style: one `[[providers]]` array entry per endpoint.
# `kind` = wire protocol, `model` = default model, `models` = model list.
# API keys are referenced by env-var name only (`api_key_env`), never stored here.
[[providers]]
name = "my-api"
type = "openai-compatible"   # or "open-router"
kind = "openai"              # or "anthropic"
base_url = "https://llm.example.com/v1"
model = "my-model-id"
api_key_env = "JCODE_PROVIDER_MY_API_API_KEY"
env_file = "provider-my-api.env"
context_window = 128000
models = ["my-model-id"]
```

Both styles parse to the same internal profile, and jcode preserves the array
style across every config save — you can keep hand-written `[[providers]]`
configs without them being rewritten to `[providers.<name>]` tables.

##### Extra request-body fields (`extra_body`)

Some OpenAI-compatible backends require non-standard top-level request fields. For example, NVIDIA NIM DeepSeek-V4 reasoning models (`deepseek-ai/deepseek-v4-flash`, `deepseek-ai/deepseek-v4-pro`) only enable thinking when the request includes `chat_template_kwargs`; without it they reply without reasoning (or, for some deployments, hang). jcode lets you inject arbitrary top-level fields two ways.

1. Per named profile, via `extra_body` in `config.toml` (a TOML table merged verbatim into the JSON body). In a resonix `[[providers]]` array entry, `extra_body` is an inline table; in a `[providers.<name>]` table it is a nested section:

   ```toml
   # Resonix array style (recommended)
   [[providers]]
   name = "my-nim"
   type = "openai-compatible"
   base_url = "https://integrate.api.nvidia.com/v1"
   model = "deepseek-ai/deepseek-v4-flash"
   api_key_env = "NVIDIA_API_KEY"
   extra_body = { chat_template_kwargs = { thinking = true, reasoning_effort = "high" } }

   # Classic table style (equivalent)
   [providers.my-nim]
   type = "openai-compatible"
   base_url = "https://integrate.api.nvidia.com/v1"
   api_key_env = "NVIDIA_API_KEY"
   default_model = "deepseek-ai/deepseek-v4-flash"

   [providers.my-nim.extra_body.chat_template_kwargs]
   thinking = true
   reasoning_effort = "high"
   ```

2. For any endpoint, via the `JCODE_OPENAI_EXTRA_BODY` environment variable (a JSON object string). It can live in the unified `~/.jcode/.env` next to the API key:

   ```bash
   JCODE_OPENAI_EXTRA_BODY={"chat_template_kwargs":{"thinking":true,"reasoning_effort":"high"}}
   ```

Keys from `extra_body` are merged last and override any jcode-generated body field with the same name (`JCODE_OPENAI_EXTRA_BODY` wins over the config `extra_body` on key collisions). Invalid values are logged and ignored rather than failing the request.

The custom OpenAI-compatible provider reads overrides from environment variables or from the unified credential file `<jcode home>/.env` (resonix-style single file; legacy per-provider env files under `~/.config/jcode/` are migrated automatically on startup).

Example for a local or LAN vLLM server:

```bash
JCODE_OPENAI_COMPAT_API_BASE=http://192.168.1.50:8000/v1
JCODE_OPENAI_COMPAT_DEFAULT_MODEL=Qwen/Qwen3-Coder-30B-A3B-Instruct
# Optional if your server expects auth
OPENAI_COMPAT_API_KEY=your-token-here
```

Notes:

- `jcode provider add <name> --base-url <url> --api-key-env <ENV_VAR>` creates or updates this for you.
- Plain `http://` is accepted for `localhost` and private LAN IPs. Public remote HTTP is still rejected.
- HTTPS endpoints work as usual.

For the direct Anthropic API-key provider, the same http/https override is available
through `JCODE_ANTHROPIC_API_BASE`, `ANTHROPIC_BASE_URL`, or `ANTHROPIC_API_BASE`.
The value may be a base like `https://host`, `https://host/v1`, or a full
`/v1/messages` URL; requests and the `/v1/models` catalog fetch use it.

#### MCP config files

MCP config is separate from `config.toml`.

Primary config files:

- `~/.jcode/mcp.json` for global MCP servers
- `.jcode/mcp.json` for project-local MCP servers

Claude Code compatibility:

- `~/.claude.json` (Claude Code's user config): top-level `mcpServers`, plus per-project servers under `projects.<abs_path>.mcpServers` for the current directory
- `.mcp.json` at the repo root (Claude Code's project config)
- `.claude/mcp.json` (legacy fallback)

Both the canonical `mcpServers` key and jcode's historical `servers` key are accepted. jcode currently supports stdio (command-based) servers only; HTTP/SSE entries (`"type": "http"`/`"sse"`) are recognized and skipped with a log line.

Example MCP config:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "/path/to/mcp-server",
      "args": ["--root", "/workspace"],
      "env": {},
      "shared": true
    }
  }
}
```

On first run, jcode also tries to import MCP servers from `~/.claude.json` (falling back to the legacy `~/.claude/mcp.json`) and `~/.codex/config.toml` if `~/.jcode/mcp.json` does not exist yet.

Inside the TUI, `/mcp` shows the live server list and `/mcp-reload`
re-reads the config files above and reconnects every server in place,
without restarting jcode.

In the input box, typing `@` at a word boundary opens a fuzzy file picker
over the session's workspace (skipping `.git`/`target`/`node_modules`);
on send, `@path` is expanded to `<@path>` plus the file content so the
model reads it directly. Pasting multi-line text never submits on its own:
2+ lines are moved to a temp file shown as a compact `@[粘贴内容N]` marker
and expanded on send, while single-line pastes stay inline.

### Supported providers

Any service that speaks OpenAI-compatible (or Anthropic Messages) chat APIs can
be connected through `jcode provider add`. Common local endpoints (Ollama,
LM Studio) and the generic OpenAI-compatible endpoint are built in as
convenience profiles; everything else is one config entry away. jcode never
hardcodes vendor names.

Existing credentials from other tools (Claude Code, Codex, OpenCode, ...) are
detected on first run and can be imported directly, so there is no re-login
required.

---

## Customizability / Self-Dev

Jcode is inventing a new form of customizability. One that doesn't limit you to what a plugin or extension can do. Tell your jcode agent to enter self dev mode, and it will start modifying its own source code. Jcode is optimized to iterate on itself. There is significant infrastructure around self developement, which allows it to edit, build, and test its own source code, then reload its own binary and continue work in your (potentially many) sessions, fully automatically. 

It is reccomended that you use a frontier model for this. The jcode codebase is not a simple one, and weaker models can make subtle, breaking changes. GPT 5.5 or the latest available frontier model works well.

<!-- Add self-dev demo thumbnail/video and fuller writeup here. -->

---

## Misc.

The devil is in the details. There are many undocumented optimizations and niceties that jcode implements. Some examples: 

Anthropic's Claude cache goes cold after 5 minutes. If you initiate Claude after these 5 minutes, you have a cache miss, potentially costing you lots of tokens. The ui warns you when the cache went cold, and notfies you if there was an unexpected cache miss. 

jcode comes with instructions on how to set up Firefox Agent Bridge. Ask you agent to set it up, and then you will have browser automation in jcode as well. 

Agent grep is a grep tool I made for the jcode agent. It adds file strucuture information (ie the list of functions, their displacement, etc) to the grep return, so that the agent can infer more of what the file doesn without actually reading the file. It also implements a harness-level integration that adaptively truncates returns based on what the agent has already seen. This saves on context a lot. 

Inputs are by default interleaved with the working agent. It sends the input as soon as it safely can without breaking the KV cache. Submit with shift enter instead, and it will send a queue send, and wait for the agent to fully finish its turn before sending.

Resume sessions from different harnesses. Claude code broke on you? Resume the session from jcode and continue where you left off. Session resume is supported for codex, claude code, opencode, and pi. 

<img width="2877" height="1762" alt="Screenshot from 2026-04-11 16-28-52" src="https://github.com/user-attachments/assets/c2b383cf-2531-4217-85ae-6a863354dc97" />
image of /Resume for codex sessions


Skills are not all loaded on startup. The conversation is embedded as a semantic vector, and will automatically inject a skill if there is an embedding hit similar to memories. The agent has a skill tool for you to manually activate a skill at anytime. You may also activate via slash commands. 

To manage skills from the TUI: `/skill` lists the currently loaded skills
and jcode-endorsed recommendations without touching disk, while
`/skill-reload` re-reads the skill directories and loads new skills into
the current session without restarting.

---

## Other planned features

Agents dont like to commit in dirty git state with active changes. Git was clearly not built for multi-agent workflows, and git worktrees is not a good solution. Given this, I believe that is an opporunity for a new git like primitive to be born. 

Build speed improvements: An incremental debug cargo build with cache enabled takes about 1 minute on my machine. The goal is 5-20 seconds. Refactors and crates seams should be able to make this happen. 

---

<div align="center">

## Quick Start

</div>

```bash
# Launch the TUI
jcode

# Run a single command non-interactively
jcode run "say hello"

# Resume a previous session by memorable name
jcode --resume fox

# Run as a persistent background server, then attach more clients
jcode serve
jcode connect

# Send voice input from your configured STT command
jcode dictate
```

jcode supports interactive TUI use, non-interactive runs, persistent server/client workflows,
and hotkey-friendly dictation without requiring a bundled speech-to-text stack.

<div align="center">

  <a href="https://github.com/1jehuang/jcode/releases/download/readme-assets/workflow.mp4">
    <img src="https://github.com/1jehuang/jcode/releases/download/readme-assets/jcode-workflow-demonstration.webp" alt="jcode workflow demonstration" width="900">
  </a>

  <p><em>jcode workflow demonstration</em></p>

</div>

---

## Browser Automation

jcode does not embed a browser tool. Browser automation is provided externally
through skills, for example the `op-browser` skill (Playwright + Chromium/Chrome
with CDP and bridge adapters), so the agent binary stays decoupled from any
specific browser or testing backend.

---

## Further Reading

- [jcode.sh/docs](https://jcode.sh/docs) — install, providers, configuration, keybindings
- [jcode.sh/swarm](https://jcode.sh/swarm) — many coding agents in one repository
- [jcode.sh/bench](https://jcode.sh/bench) — benchmark methodology and results
- [环境模式 / OpenClaw](docs/环境模式.md)
- [记忆架构](docs/记忆架构.md)
- [集群架构](docs/集群架构.md)
- [服务器架构](docs/服务器架构.md)
- [安全系统](docs/安全系统.md)
- [赞助发现与赞助方接入](docs/赞助发现与赞助方接入.md)
- [Windows 说明](docs/Windows平台.md)
- [包装脚本与 Shell 集成](docs/包装脚本指南.md)
- [重构说明](docs/重构路线图.md)

---

## Detailed Installation

### Setup

If you want another agent to set up jcode for you, give it this prompt:

```text
Set up jcode on this machine for me.

1. Detect the operating system, available package managers, and shell environment, then install jcode using the best matching command below instead of referring me somewhere else:

   - macOS with Homebrew available:
     brew tap 1jehuang/jcode
     brew install jcode

   - macOS or Linux via install script:
     curl -fsSL https://jcode.sh/install | bash

   - Windows PowerShell:
     irm https://jcode.sh/install.ps1 | iex

   - From source if the above paths are not appropriate:
     git clone https://github.com/1jehuang/jcode.git
     cd jcode
     cargo build --release
     scripts/install_release.sh

   - For local self-dev / refactor work on Linux x86_64, prefer:
     scripts/dev_cargo.sh build --release -p jcode --bin jcode
     scripts/dev_cargo.sh --print-setup
     scripts/install_release.sh

2. Verify that `jcode` is on my `PATH`.
3. Launch `jcode` once in a new terminal window/session to confirm it starts successfully.
4. Before attempting any interactive login flow, assess which providers are already available non-interactively and prefer those first. Check existing local credentials, config files, CLI sessions, and environment variables such as:
   - Claude: `~/.jcode/auth.json`, `~/.claude/.credentials.json`, `~/.local/share/opencode/auth.json`, `ANTHROPIC_API_KEY`
   - OpenAI: `~/.jcode/openai-auth.json`, `~/.codex/auth.json`, `OPENAI_API_KEY`
   - Gemini: `~/.jcode/gemini_oauth.json`, `~/.gemini/oauth_creds.json`
   - GitHub Copilot: existing auth under `~/.config/github-copilot/`
   - Azure OpenAI: `~/.config/jcode/azure-openai.env`, `AZURE_OPENAI_*`, or an existing `az login`
   - OpenRouter: `OPENROUTER_API_KEY`
   - Fireworks: `~/.config/jcode/fireworks.env`, `FIREWORKS_API_KEY`
   - MiniMax: `~/.config/jcode/minimax.env`, `MINIMAX_API_KEY`
   - NVIDIA NIM: `~/.config/jcode/nvidia-nim.env`, `NVIDIA_API_KEY`
   - Alibaba Cloud Coding Plan: existing jcode config/env if present
5. Prefer whichever provider is already configured and verify it with `jcode auth-test --all-configured` or a provider-specific auth test when appropriate.
6. Only if no usable provider is already configured, guide me through the minimal manual step needed:
   - Any OpenAI-compatible endpoint: `jcode provider add <name> --base-url <url> --model <id> --api-key-env <ENV_VAR>`
   - Ollama (local): `jcode provider add ollama-local --base-url http://localhost:11434/v1 --model llama3.2 --no-api-key`
   - LM Studio (local): `jcode provider add lmstudio --base-url http://localhost:1234/v1 --model <model-id> --no-api-key`
   - OpenRouter: `jcode provider add openrouter --base-url https://openrouter.ai/api/v1 --model <id> --api-key-env OPENROUTER_API_KEY`
   - Anthropic direct API: `jcode provider add anthropic --base-url https://api.anthropic.com/v1 --model <id> --api-key-env ANTHROPIC_API_KEY`
   - Store the key value in the unified `~/.jcode/.env` (`ENV_VAR=value`), never in `config.toml`
7. After setup, run a simple smoke test with `jcode run "say hello"` and confirm it works.
8. If I want browser automation, use an external skill such as `op-browser` (Playwright + Chromium/Chrome); jcode itself does not embed a browser tool.
9. Explain any manual step that still needs me, especially browser OAuth, device login, API key entry, or browser extension approval.
```

This is intended to be a copy-paste bootstrap prompt for jcode itself or any other coding agent.

### Quick Install

```bash
# macOS & Linux
curl -fsSL https://jcode.sh/install | bash
```

On Termux, install the glibc runtime and `patchelf` first so the installer can
patch the downloaded Linux binary to Termux's glibc dynamic linker and create a
launcher that avoids Termux's `LD_PRELOAD` shim:

```bash
pkg install glibc patchelf
curl -fsSL https://jcode.sh/install | bash
```

```powershell
# Windows 11 x64 or ARM64 (PowerShell 5.1+)
irm https://jcode.sh/install.ps1 | iex
```

The Windows installer selects the correct architecture and verifies the download
against the release's `SHA256SUMS`. Alacritty and the optional global launch
hotkey require explicit consent and are not installed by default. See
[Windows 支持、安全、Defender 和 SmartScreen 说明](docs/Windows平台.md).

If a release does not contain a matching Windows asset, the installer stops
instead of unexpectedly starting a long compilation. An explicit source build
is available with `-BuildFromSource` and requires Git, Rust, and Visual Studio
2022 Build Tools with the **Desktop development with C++** workload.

### Install / Update from a Local Package (Offline)

To install or update jcode entirely from a locally provided package without
touching the official site or GitHub, pass the package path directly:

- **Already running jcode** (`.tar.gz` archive or a bare `.exe`/ELF binary):

  ```bash
  jcode update --local /path/to/jcode-windows-x86_64-<hash>.tar.gz
  jcode update --local /path/to/jcode-windows-x86_64-<hash>.exe
  ```

  The package is extracted (`.tar.gz`) or staged (bare binary), probed for its
  version, archived under `builds/versions/<version>/`, and the `stable` /
  `current` channels plus the launcher are switched to it. The whole run is
  offline: no release lookup, no download, no checksum fetch.

- **Fresh install on Windows** (PowerShell):

  ```powershell
  .\install.ps1 -ArtifactExePath C:\path\to\jcode-windows-x86_64-<hash>.exe
  .\install.ps1 -ArtifactTgzPath C:\path\to\jcode-windows-x86_64-<hash>.tar.gz
  ```

- **Fresh install on macOS / Linux** (shell):

  ```bash
  JCODE_LOCAL_ARTIFACT=/path/to/jcode-linux-x86_64-<hash>.tar.gz ./install.sh
  ```

Local packages often carry a git hash suffix (e.g.
`jcode-windows-x86_64-2dc3213a6.exe`); the installer and `jcode update --local`
detect and install them the same way as official assets.

### macOS via Homebrew

```bash
brew tap 1jehuang/jcode
brew install jcode
```

### From Source (all platforms)

```bash
git clone https://github.com/1jehuang/jcode.git
cd jcode
cargo build --release
```

For local self-dev / refactor work on Linux x86_64, prefer:

```bash
scripts/dev_cargo.sh build --release -p jcode --bin jcode
scripts/dev_cargo.sh --print-setup
```

That wrapper automatically uses `sccache` when available, prefers a fast
working local linker setup (`clang + lld`) instead of assuming every machine's
`mold` configuration is valid, and can print the active linker/cache setup via
`--print-setup` so slow-path builds are easier to diagnose.

Then symlink to your PATH:

```bash
scripts/install_release.sh
```

### Uninstall

Removes installed binaries and the launcher but keeps your config, auth, and
sessions so a clean reinstall picks up where you left off:

```bash
curl -fsSL https://raw.githubusercontent.com/1jehuang/jcode/master/scripts/uninstall.sh | bash -s -- --yes
```

For a full wipe of everything including config, auth, sessions, logs, and
memory (useful for recovering from a broken install):

```bash
curl -fsSL https://raw.githubusercontent.com/1jehuang/jcode/master/scripts/uninstall.sh | bash -s -- --purge --yes
```

Add `--dry-run` to preview what would be removed without deleting anything.

### Platform Support

| Platform | Status |
|---|---|
| **Linux** x86_64 / aarch64 | Fully supported |
| **macOS** Apple Silicon & Intel | Supported |
| **Windows** x86_64 | Supported (native + WSL2) |
| **Termux** aarch64 / x86_64 | Supported with `pkg install glibc patchelf` |

</div>
