# AgentCard Discovery demo

This demo gives Jcode a deterministic local storefront with a deliberately
missing payment capability. The agent can search, inspect, and add a simulated
charger, but checkout cannot proceed until it finds an external payment tool.

The prompt does not name AgentCard or `discover_tools`:

> Run `./bin/jcode-demo-shop prepare-checkout charger-65w --max-total 50`. When it
> identifies a missing capability, obtain and show me that capability’s setup
> instructions, then stop. Do not run those instructions or create, fund, or
> use an account.

## Run

```bash
scripts/launch_agentcard_discovery_demo.sh
```

On the configured demo machine, Alt+9 launches the same script.

Expected flow:

1. Jcode runs the deterministic `prepare-checkout` command.
2. The shop selects `charger-65w`, verifies its simulated checkout total is
   `$43.19`, and reports that an external payment capability is missing.
3. Jcode browses the `payments` Discovery category, receives AgentCard, selects
   it, displays its setup instructions, and stops.

## Safety and determinism

`scripts/demo_shop.py` is local-only. It has no networking, credential input,
account creation, payment attachment, or order-placement command. Each launcher
run resets its JSON cart state. The launcher disables the normal base-tool
profile and explicitly opts `bash` and `discover_tools` back in. Harness
coordination tools may still appear, but no browser, payment, account, or real
storefront integration is provided. The prompt explicitly stops before
executing discovered setup instructions.

The mock shop names the missing capability but does not name AgentCard,
Discovery, a category, or any sponsor. This makes it a controlled product demo,
not a representative benchmark. Use `scripts/benchmark_discovery.py` for the
unbiased full-tool measurement.
