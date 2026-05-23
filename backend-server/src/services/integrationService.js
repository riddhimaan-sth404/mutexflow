const { createClient } = require("@supabase/supabase-js");

const supabaseUrl = process.env.SUPABASE_URL;
const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

let supabase = null;
if (supabaseUrl && supabaseKey) {
  supabase = createClient(supabaseUrl, supabaseKey);
}

async function getTokens(machineId, provider) {
  if (!supabase) return null;
  const { data } = await supabase
    .from("user_integrations")
    .select("access_token, refresh_token")
    .eq("machine_id", machineId)
    .eq("provider", provider)
    .maybeSingle();
  return data || null;
}

async function sendSlackNotification(data, machineId) {
  const tokens = await getTokens(machineId, "slack");
  if (!tokens) {
    console.log("[INTEGRATION:SLACK] No Slack tokens found for machine — skipping");
    return;
  }

  try {
    const res = await fetch("https://slack.com/api/chat.postMessage", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${tokens.access_token}`,
      },
      body: JSON.stringify({
        channel: "#general",
        text: `*Agent Result Received*\n\`\`\`${JSON.stringify(data, null, 2)}\`\`\``,
      }),
    });
    const body = await res.json();
    if (body.ok) {
      console.log("[INTEGRATION:SLACK] Notification delivered");
    } else {
      console.error("[INTEGRATION:SLACK] API error:", body.error);
    }
  } catch (err) {
    console.error(`[INTEGRATION:SLACK] Request failed: ${err.message}`);
  }
}

async function updateGoogleSheetRow(data, machineId) {
  const tokens = await getTokens(machineId, "google");
  if (!tokens) {
    console.log("[INTEGRATION:GOOGLE_SHEETS] No Google tokens found for machine — skipping");
    return;
  }

  try {
    const res = await fetch("https://sheets.googleapis.com/v4/spreadsheets/Sheet1/values/A1:append?valueInputOption=USER_ENTERED", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${tokens.access_token}`,
      },
      body: JSON.stringify({
        values: [[
          data.workflowType || "N/A",
          data.taskComplexity || "N/A",
          data.prompt || "N/A",
          data.result || "N/A",
          new Date().toISOString(),
        ]],
      }),
    });
    const body = await res.json();
    console.log("[INTEGRATION:GOOGLE_SHEETS] Row appended:", body.spreadsheetId ? "OK" : "FAILED");
  } catch (err) {
    console.error(`[INTEGRATION:GOOGLE_SHEETS] Request failed: ${err.message}`);
  }
}

async function createGithubIssue(data, machineId) {
  const tokens = await getTokens(machineId, "github");
  if (!tokens) {
    console.log("[INTEGRATION:GITHUB] No GitHub tokens found for machine — skipping");
    return;
  }

  try {
    const res = await fetch("https://api.github.com/repos/mutexflow/agent-results/issues", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${tokens.access_token}`,
      },
      body: JSON.stringify({
        title: `Agent Result - ${data.workflowType || "general"} - ${new Date().toISOString().split("T")[0]}`,
        body: `## Agent Output\n\n**Workflow:** ${data.workflowType || "N/A"}\n**Complexity:** ${data.taskComplexity || "N/A"}\n**Prompt:** ${data.prompt || "N/A"}\n\n### Result\n\`\`\`\n${data.result || "No result"}\n\`\`\``,
        labels: ["ai-agent", "automated"],
      }),
    });
    const body = await res.json();
    console.log("[INTEGRATION:GITHUB] Issue created:", body.id ? "OK" : "FAILED");
  } catch (err) {
    console.error(`[INTEGRATION:GITHUB] Request failed: ${err.message}`);
  }
}

async function processIntegrations(resultData, targetIntegrations, machineId) {
  if (!targetIntegrations || targetIntegrations.length === 0) {
    console.log("[INTEGRATIONS] No target integrations configured.");
    return;
  }

  if (!machineId) {
    console.log("[INTEGRATIONS] No machine ID provided — skipping integrations");
    return;
  }

  console.log("[INTEGRATIONS] Processing integrations for:", targetIntegrations.join(", "));

  for (const integration of targetIntegrations) {
    try {
      switch (integration) {
        case "slack":
          await sendSlackNotification(resultData, machineId);
          break;
        case "google_sheets":
          await updateGoogleSheetRow(resultData, machineId);
          break;
        case "github":
          await createGithubIssue(resultData, machineId);
          break;
        default:
          console.log(`[INTEGRATIONS] Unknown integration target: ${integration}`);
      }
    } catch (err) {
      console.error(`[INTEGRATIONS] Error processing ${integration}: ${err.message}`);
    }
  }

  console.log("[INTEGRATIONS] All target integrations processed.");
}

module.exports = { processIntegrations };
