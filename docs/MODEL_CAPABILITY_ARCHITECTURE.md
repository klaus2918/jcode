# Model Capability Architecture

> Status: implemented (P0) as of v0.64.2-dev
> Design reference: DeepSeek-Reasonix 000-task docs 01/03 (model access
> architecture) adapted to the jcode Rust workspace.

jcode's model-access architecture is layered as:

```text
config layer (jcode-config-types + jcode-base/config)
    |  NamedProviderConfig / NamedProviderModelConfig (capability fields)
    v
capability layer (jcode-provider-core::capability)
    |  ModelCapability + embedded registry + merge pipeline
    |  priority: explicit config > registry > heuristics > conservative default
    v
resolution layer (jcode-base provider/models + catalog_routes)
    |  ModelRoute capability projection (modalities/tools/reasoning/window/sampling)
    v
adapter layer (jcode-provider-*-runtime)
    |  Provider trait + optional capability interfaces
    v
runtime layer (jcode-app-core agent)
    |  tool gating, vision gating, effort, compaction, recovery
```

## 1. Capability model (`jcode-provider-core/src/capability.rs`)

`ModelCapability` is the single source of truth for declarative model
capabilities:

- `chat: bool`
- `modalities: Vec<Modality>` (`text` is implicit; `image`/`audio` are additive)
- `reasoning: ReasoningCapability` (`protocol`, `efforts`, `default_effort`,
  `round_trip`, `tool_call_replay`, `thinking_kind`)
- `tools: Option<bool>` (`None` = unknown -> tools available)
- `context_window` / `output_window`
- `sampling: SamplingCapability` (`temperature_supported`, `fixed_sampling`,
  `output_limit_field`)

The embedded `EMBEDDED_REGISTRY` is a `const` table of `RegistryEntry` values
(exact ids plus `claude-*`/`gpt-5*` prefix entries, optionally scoped by
provider key). Entries are seeded to mirror the previous heuristic outputs so
the registry does not change behavior on its own; it adds fields heuristics
cannot express (vision, tools, sampling, reasoning round-trip flags).

## 2. Resolution priority

Every capability decision goes through
`resolve_capability_with_context_fn` (or `resolve_capability` for the
provider-core default heuristic):

```text
1. explicit config : NamedProviderModelConfig fields / input modalities
2. embedded registry: EMBEDDED_REGISTRY match (exact > prefix, provider-scoped)
3. heuristics      : open-weight context families, reasoning ladders, vision names
4. conservative default: text-only, tools available, no reasoning control
```

`ResolvedModelCapability` carries a `CapabilityTrace` recording the winning
source per field (`config` / `registry` / `heuristic` / `default`) for
diagnostics.

## 3. Configuration surface (`jcode-config-types`)

Per-model overrides under `[providers.<name>.models]`:

```toml
[[providers.my-gateway.models]]
id                   = "custom-vlm"
context_window       = 128000
input                = ["text", "image"]
vision               = true            # explicit visual capability
tools                = false           # never send tool definitions
reasoning_protocol   = "openai"        # auto|deepseek|openai|thinking_type|anthropic|none
supported_efforts    = ["low", "high"] # /effort ladder
default_effort       = "high"          # /effort auto target
output_window        = 16384
temperature_supported = false          # fixed-sampling backends
fixed_sampling       = true
output_limit_field   = "max_completion_tokens"
```

All fields are optional and aliased (`context_limit`, `supports_vision`,
`efforts`, `reasoning-protocol`, ...); old configs parse unchanged.

## 4. Route projection

`ModelRoute` gains an optional `capability: Option<RouteCapabilityView>`
(serde `skip_serializing_if`), so remote clients and the model picker see
declared capabilities without touching credentials or endpoints. Views that
carry only the conservative default are omitted, keeping wire bytes stable.

`jcode model list --json` includes the capability view per route;
`jcode model list --verbose` prints a human-readable capability block.

## 5. Gating

- **Tool gating**: agent turns call `gated_tool_definitions()`, which clears
  the toolset when the active model resolves `tools == false` (explicit config
  or registry). Unknown models keep tools (no behavior change).
- **Vision gating**: `capability.modalities` includes `image` for image-capable
  models; runtimes keep their existing image handling.
- **Sampling**: `SamplingCapability` generalizes the Kimi K3 fixed-sampling
  constraint; explicit `temperature_supported`/`fixed_sampling`/
  `output_limit_field` fields feed the route view.

## 6. Onboarding a new model

### Same protocol, new vendor/model (most common)

1. Add/verify the endpoint under `[providers.<name>]` with
   `api_key_env` (never paste keys into TOML).
2. Declare the model and its capability fields under `[[providers.<name>.models]]`.
3. If the model needs no explicit overrides, add an embedded registry entry
   (or wait for the heuristic to cover it).
4. Verify with `jcode model list --json` (route capability) and a live turn.

### New provider kind (new wire protocol)

1. Add a format crate (`jcode-provider-<name>`) and a runtime crate
   (`jcode-provider-<name>-runtime`) implementing `Provider`.
2. Register the runtime in `src/cli/startup.rs` alongside the existing
   external runtimes.
3. Add wire-shape reasoning/effort handling in the runtime; declare the
   protocol in the registry when it is a known shape.
4. Add route building in `jcode-base/src/provider/catalog_routes.rs` and
   credentials/auth handling in the auth layer.
5. Add tests mirroring `jcode-provider-openai-runtime` /
   `jcode-provider-anthropic-runtime` coverage (request construction, SSE,
   tool streaming, error mapping, retries).
6. Document the endpoint and model in this file and the provider metadata
   catalog (`jcode-provider-metadata`).

## 7. Compatibility guarantees

- New config fields are optional with aliases; old TOML loads unchanged.
- `ModelRoute.capability` is skipped when empty/default; old clients ignore
  unknown fields.
- The registry only affects *default* resolution; explicit config always wins.
- No credential, endpoint, or proxy data ever enters `ModelCapability` or
  `RouteCapabilityView`; both are safe to send to remote clients.
