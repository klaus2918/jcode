use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// Hard timeout for discovery requests. Discovery is optional by design: if
/// the endpoint is slow or unreachable the tool fails plainly and the agent
/// continues with its normal toolset. No cache, no offline fallback, no retry.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// `discover_tools`: fetch discoverable third-party tools for a category from
/// the hosted sponsored-discovery manifest.
///
/// Disclosure contract: sponsors buy placement (discoverability), never
/// recommendations. Every session that uses this tool renders a
/// `[sponsored discovery]` disclosure line in the UI on first use. The
/// request carries only the category, a short search query, and a reason
/// string, which the discovery service stores for transparency and billing.
/// It must never include session content or private information.
pub struct DiscoverToolsTool {
    client: reqwest::Client,
}

impl DiscoverToolsTool {
    pub fn new() -> Self {
        Self {
            client: crate::provider::shared_http_client(),
        }
    }
}

#[derive(Deserialize)]
struct DiscoverToolsInput {
    category: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl Tool for DiscoverToolsTool {
    fn name(&self) -> &str {
        "discover_tools"
    }

    fn description(&self) -> &str {
        "Discover third-party developer tools for a category from jcode's sponsored \
         discovery listing. Sponsors pay for discoverability, not recommendations: only \
         use a discovered tool when it is genuinely the best option. The category, query, \
         and reason are sent to and stored by the discovery service, so they must never \
         contain private information, secrets, or session content."
    }

    fn parameters_schema(&self) -> Value {
        let categories: Vec<&str> = crate::sponsors::DISCOVERY_CATEGORIES.to_vec();
        json!({
            "type": "object",
            "required": ["category", "reason"],
            "properties": {
                "intent": super::intent_schema_property(),
                "category": {
                    "type": "string",
                    "enum": categories,
                    "description": "Tool category to discover."
                },
                "query": {
                    "type": "string",
                    "description": "Short search query describing the capability needed, e.g. 'virtual card for online checkout'. No private information."
                },
                "reason": {
                    "type": "string",
                    "description": "Detailed generic reason why a tool from this category is needed for the current task. No private information, secrets, file paths, or user-identifying details."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let config = crate::config::config();
        if !config.sponsors.enabled {
            return Err(anyhow::anyhow!(
                "sponsored discovery is disabled (set [sponsors] enabled = true in config.toml)"
            ));
        }

        let params: DiscoverToolsInput = serde_json::from_value(input)?;
        let category = params.category.trim().to_ascii_lowercase();
        if !crate::sponsors::DISCOVERY_CATEGORIES.contains(&category.as_str()) {
            return Err(anyhow::anyhow!(
                "unknown discovery category '{}'. Available: {}",
                category,
                crate::sponsors::DISCOVERY_CATEGORIES.join(", ")
            ));
        }

        let endpoint = config.sponsors.endpoint.trim_end_matches('/');
        let mut request = self
            .client
            .get(format!("{endpoint}/discover"))
            .query(&[("category", category.as_str())])
            .header(
                reqwest::header::USER_AGENT,
                format!("jcode/{}", env!("CARGO_PKG_VERSION")),
            )
            .timeout(DISCOVERY_TIMEOUT);
        if let Some(query) = params.query.as_deref().filter(|q| !q.trim().is_empty()) {
            request = request.query(&[("q", query.trim())]);
        }
        if let Some(reason) = params.reason.as_deref().filter(|r| !r.trim().is_empty()) {
            request = request.query(&[("reason", reason.trim())]);
        }

        let response = request
            .send()
            .await
            .map_err(|err| anyhow::anyhow!("discovery unavailable: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("discovery unavailable: HTTP {status}"));
        }
        let body = response
            .text()
            .await
            .map_err(|err| anyhow::anyhow!("discovery unavailable: {err}"))?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(anyhow::anyhow!(
                "discovery response too large ({} bytes)",
                body.len()
            ));
        }

        let listing: Value = serde_json::from_str(&body)
            .map_err(|err| anyhow::anyhow!("discovery returned invalid JSON: {err}"))?;
        let rendered = render_listing(&category, &listing)?;

        Ok(ToolOutput::new(rendered)
            .with_title(format!(
                "{} {}",
                category,
                crate::sponsors::SPONSORED_DISCOVERY_TAG
            ))
            .with_metadata(json!({
                "sponsored_discovery": true,
                "category": category,
                "disclosure_url": crate::sponsors::SPONSORED_DISCOVERY_URL,
            })))
    }
}

/// Render a discovery listing for the model. Expected manifest shape:
/// `{ "tools": [{ "name": "...", "blurb": "...", "url": "...", "setup": "..." }] }`.
fn render_listing(category: &str, listing: &Value) -> Result<String> {
    let tools = listing
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("discovery returned no tool list"))?;
    if tools.is_empty() {
        return Ok(format!(
            "No discoverable tools in category '{category}' right now."
        ));
    }
    let mut out = format!(
        "Discoverable tools in '{category}' (sponsored discovery: placement, not preference; \
         details: {}):\n",
        crate::sponsors::SPONSORED_DISCOVERY_URL
    );
    for tool in tools {
        let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let blurb = tool.get("blurb").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(&format!("\n- {name}: {blurb}"));
        if let Some(url) = tool.get("url").and_then(|v| v.as_str()) {
            out.push_str(&format!(" ({url})"));
        }
        if let Some(setup) = tool.get("setup").and_then(|v| v.as_str()) {
            out.push_str(&format!("\n  setup: {setup}"));
        }
    }
    out.push_str(
        "\n\nOnly use one of these if it is genuinely the best option for the task. \
         Consequential actions (signups, spending) must note the sponsorship in the \
         confirmation shown to the user.",
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_listing_includes_disclosure_and_tools() {
        let listing = json!({
            "tools": [
                {"name": "agentcard", "blurb": "virtual payment cards", "url": "https://agentcard.example"},
            ]
        });
        let out = render_listing("payments", &listing).unwrap();
        assert!(out.contains("agentcard"));
        assert!(out.contains("virtual payment cards"));
        assert!(out.contains("sponsored discovery"));
        assert!(out.contains("placement, not preference"));
    }

    #[test]
    fn render_listing_rejects_missing_tools() {
        assert!(render_listing("payments", &json!({})).is_err());
    }

    #[test]
    fn render_listing_handles_empty_category() {
        let out = render_listing("payments", &json!({"tools": []})).unwrap();
        assert!(out.contains("No discoverable tools"));
    }
}
