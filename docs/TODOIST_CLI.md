# Todoist CLI

A small Rust CLI for the Todoist REST API.

## Build

```bash
cargo build --bin todoist
```

## Auth

Set a Todoist API token in an environment variable:

```bash
export TODOIST_API_TOKEN=your_token_here
```

You can also point the CLI at a different env var:

```bash
todoist --token-env MY_TODOIST_TOKEN projects list
```

## Commands

```bash
# list projects
todoist projects list
todoist projects ls

# list tasks
todoist tasks list
todoist tasks ls --filter 'today'
todoist tasks ls --project Inbox

# create a task
todoist tasks add "Ship release notes" --project Work --due 'tomorrow 9am' --priority 4 --label release

# inspect a task
todoist tasks show TASK_ID

# complete or delete a task
todoist tasks done TASK_ID
todoist tasks delete TASK_ID
```

## JSON mode

```bash
todoist --json tasks ls --filter 'today'
```

## Testing with a mock server

The CLI supports `--base-url` so it can be tested against a local fake Todoist API.
