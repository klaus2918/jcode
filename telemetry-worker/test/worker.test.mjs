// Tests for the telemetry worker's dual-write + D1 self-defense behavior.
// Run with: node --test test/
//
// The worker module is plain ESM with injected bindings (env.DB, env.FIREHOSE),
// so it can be exercised without wrangler by passing mocks.
import test from "node:test";
import assert from "node:assert/strict";

import worker from "../src/worker.js";

const EVENT_URL = "https://telemetry.example/v1/event";
const HEALTH_URL = "https://telemetry.example/v1/health";

function makeBody(overrides = {}) {
  return {
    id: "11111111-2222-3333-4444-555555555555",
    event: "onboarding_step",
    version: "0.0.0-test",
    os: "linux",
    arch: "x86_64",
    step: "auth_failed",
    auth_provider: "testprov",
    auth_method: "oauth",
    auth_failure_reason: "callback_timeout",
    ...overrides,
  };
}

function postRequest(body, url = EVENT_URL) {
  return new Request(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

// Minimal D1 mock. `plan` lets tests fail specific statements or set the
// reported database size.
function makeDb(plan = {}) {
  const executed = [];
  const sizeAfter = plan.sizeAfter ?? 1000;
  return {
    executed,
    prepare(sql) {
      return {
        bind(...values) {
          return {
            async run() {
              executed.push({ sql, values });
              if (plan.failInserts && /^INSERT/i.test(sql.trim())) {
                throw new Error(plan.failureMessage || "generic transient error");
              }
              return { meta: { changes: 1, size_after: sizeAfter } };
            },
            async all() {
              executed.push({ sql, values });
              return { results: [] };
            },
          };
        },
        async run() {
          executed.push({ sql, values: [] });
          return { meta: { changes: 0, size_after: sizeAfter } };
        },
        async all() {
          executed.push({ sql, values: [] });
          // PRAGMA table_info: report every column the worker may reference.
          if (/table_info/.test(sql)) {
            return {
              results: [
                "telemetry_id", "event", "version", "os", "arch", "step",
                "auth_provider", "auth_method", "auth_failure_reason",
                "milestone_elapsed_ms", "event_id", "session_id",
                "schema_version", "build_channel", "is_git_checkout", "is_ci",
                "ran_from_cargo",
              ].map((name) => ({ name })),
            };
          }
          return { results: [] };
        },
      };
    },
  };
}

function makeFirehose() {
  const points = [];
  return {
    points,
    writeDataPoint(point) {
      points.push(point);
    },
  };
}

function makeCtx() {
  const waited = [];
  return {
    waited,
    waitUntil(promise) {
      waited.push(promise);
    },
  };
}

test("event is dual-written: firehose point + D1 insert", async () => {
  const db = makeDb();
  const firehose = makeFirehose();
  const ctx = makeCtx();

  const response = await worker.fetch(postRequest(makeBody()), { DB: db, FIREHOSE: firehose }, ctx);
  const json = await response.json();

  assert.equal(response.status, 200);
  assert.equal(json.ok, true);
  assert.equal(json.durable, true);
  assert.equal(json.firehose, true);

  assert.equal(firehose.points.length, 1);
  const point = firehose.points[0];
  // index1 = telemetry_id (sampling key)
  assert.deepEqual(point.indexes, ["11111111-2222-3333-4444-555555555555"]);
  // FIREHOSE_SCHEMA blob positions (append-only contract):
  assert.equal(point.blobs[0], "onboarding_step"); // blob1 = event
  assert.equal(point.blobs[7], "auth_failed"); // blob8 = step
  assert.equal(point.blobs[8], "testprov"); // blob9 = auth_provider
  assert.equal(point.blobs[10], "callback_timeout"); // blob11 = auth_failure_reason
  assert.equal(point.blobs.length, 20);
  assert.equal(point.doubles.length, 20);

  assert.ok(db.executed.some(({ sql }) => /INSERT OR IGNORE INTO events/.test(sql)));
});

test("D1 failure with firehose success degrades to durable:false instead of 500", async () => {
  const db = makeDb({ failInserts: true });
  const firehose = makeFirehose();
  const ctx = makeCtx();

  const response = await worker.fetch(postRequest(makeBody()), { DB: db, FIREHOSE: firehose }, ctx);
  const json = await response.json();

  assert.equal(response.status, 200);
  assert.equal(json.ok, true);
  assert.equal(json.durable, false);
  assert.equal(json.firehose, true);
  assert.equal(firehose.points.length, 1);
});

test("SQLITE_FULL-class insert failure schedules an emergency prune", async () => {
  const db = makeDb({ failInserts: true, failureMessage: "SQLITE_FULL: database or disk is full" });
  const firehose = makeFirehose();
  const ctx = makeCtx();

  await worker.fetch(postRequest(makeBody()), { DB: db, FIREHOSE: firehose }, ctx);
  // The prune is scheduled via ctx.waitUntil; drain it and check DELETEs ran.
  await Promise.all(ctx.waited);

  assert.ok(
    db.executed.some(({ sql }) => /DELETE FROM events/.test(sql)),
    "emergency prune should issue DELETEs after a full-database failure",
  );
});

test("D1 failure without firehose binding still returns 500", async () => {
  const db = makeDb({ failInserts: true, failureMessage: "some transient error" });
  const ctx = makeCtx();

  const response = await worker.fetch(postRequest(makeBody()), { DB: db }, ctx);
  assert.equal(response.status, 500);
});

test("missing firehose binding degrades gracefully", async () => {
  const db = makeDb();
  const ctx = makeCtx();

  const response = await worker.fetch(postRequest(makeBody()), { DB: db }, ctx);
  const json = await response.json();

  assert.equal(response.status, 200);
  assert.equal(json.ok, true);
  assert.equal(json.durable, true);
  assert.equal(json.firehose, false);
});

test("health endpoint reports database size vs soft limit", async () => {
  const db = makeDb({ sizeAfter: 12345678 });
  const ctx = makeCtx();

  const response = await worker.fetch(new Request(HEALTH_URL, { method: "GET" }), { DB: db }, ctx);
  const json = await response.json();

  assert.equal(response.status, 200);
  assert.equal(json.ok, true);
  assert.equal(json.db_size_bytes, 12345678);
  assert.equal(typeof json.db_soft_limit_bytes, "number");
  assert.equal(json.over_soft_limit, false);
});

test("unknown event type is rejected", async () => {
  const db = makeDb();
  const ctx = makeCtx();
  const response = await worker.fetch(
    postRequest(makeBody({ event: "mystery" })),
    { DB: db },
    ctx,
  );
  assert.equal(response.status, 400);
});
