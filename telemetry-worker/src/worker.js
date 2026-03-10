export default {
  async fetch(request, env) {
    if (request.method === "OPTIONS") {
      return new Response(null, {
        headers: corsHeaders(),
      });
    }

    if (request.method !== "POST") {
      return jsonResponse({ error: "Method not allowed" }, 405);
    }

    const url = new URL(request.url);
    if (url.pathname !== "/v1/event") {
      return jsonResponse({ error: "Not found" }, 404);
    }

    let body;
    try {
      body = await request.json();
    } catch {
      return jsonResponse({ error: "Invalid JSON" }, 400);
    }

    if (!body.id || !body.event || !body.version || !body.os || !body.arch) {
      return jsonResponse({ error: "Missing required fields" }, 400);
    }

    if (!["install", "session_end"].includes(body.event)) {
      return jsonResponse({ error: "Unknown event type" }, 400);
    }

    try {
      if (body.event === "install") {
        await env.DB.prepare(
          `INSERT INTO events (telemetry_id, event, version, os, arch)
           VALUES (?, ?, ?, ?, ?)`
        )
          .bind(body.id, body.event, body.version, body.os, body.arch)
          .run();
      } else if (body.event === "session_end") {
        const errors = body.errors || {};
        await env.DB.prepare(
          `INSERT INTO events (
            telemetry_id, event, version, os, arch,
            provider_start, provider_end, model_start, model_end,
            provider_switches, model_switches, duration_mins, turns,
            error_provider_timeout, error_auth_failed, error_tool_error,
            error_mcp_error, error_rate_limited
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
        )
          .bind(
            body.id,
            body.event,
            body.version,
            body.os,
            body.arch,
            body.provider_start || null,
            body.provider_end || null,
            body.model_start || null,
            body.model_end || null,
            body.provider_switches || 0,
            body.model_switches || 0,
            body.duration_mins || 0,
            body.turns || 0,
            errors.provider_timeout || 0,
            errors.auth_failed || 0,
            errors.tool_error || 0,
            errors.mcp_error || 0,
            errors.rate_limited || 0
          )
          .run();
      }

      return jsonResponse({ ok: true });
    } catch (err) {
      return jsonResponse({ error: "Internal error" }, 500);
    }
  },
};

function jsonResponse(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "Content-Type": "application/json",
      ...corsHeaders(),
    },
  });
}

function corsHeaders() {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  };
}
