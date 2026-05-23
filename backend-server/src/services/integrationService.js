const querystring = require("querystring");
const { supabase } = require("../config/db");

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

const REFRESH_ENDPOINTS = {
  google: "https://oauth2.googleapis.com/token",
  microsoft: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
};

async function refreshAccessToken(machineId, provider, currentRefreshToken) {
  const tokenUrl = REFRESH_ENDPOINTS[provider];
  if (!tokenUrl || !currentRefreshToken) return null;

  const envPrefix = provider.toUpperCase();
  const clientId = process.env[`${envPrefix}_CLIENT_ID`];
  const clientSecret = process.env[`${envPrefix}_CLIENT_SECRET`];
  if (!clientId || !clientSecret) return null;

  try {
    const res = await fetch(tokenUrl, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: querystring.stringify({
        client_id: clientId,
        client_secret: clientSecret,
        refresh_token: currentRefreshToken,
        grant_type: "refresh_token",
      }),
    });

    const data = await res.json();
    if (!data.access_token) {
      console.error(`[REFRESH:${provider}] Refresh failed:`, data);
      return null;
    }

    if (supabase) {
      await supabase
        .from("user_integrations")
        .update({
          access_token: data.access_token,
          refresh_token: data.refresh_token || currentRefreshToken,
          updated_at: new Date(),
        })
        .eq("machine_id", machineId)
        .eq("provider", provider);
    }

    console.log(`[REFRESH:${provider}] Token refreshed successfully`);
    return data.access_token;
  } catch (err) {
    console.error(`[REFRESH:${provider}] Couldn't refresh token: ${err.message}`);
    return null;
  }
}

async function getAccessToken(machineId, provider, allowRefresh = true) {
  const tokens = await getTokens(machineId, provider);
  if (!tokens) return null;

  let token = tokens.access_token;

  if (allowRefresh && REFRESH_ENDPOINTS[provider] && tokens.refresh_token) {
    const refreshed = await refreshAccessToken(machineId, provider, tokens.refresh_token);
    if (refreshed) token = refreshed;
  }

  return token;
}

async function sendSlackNotification(data, machineId) {
  const token = await getAccessToken(machineId, "slack", false);
  if (!token) {
    console.log("[INTEGRATION:SLACK] No Slack connection found — skipping");
    return;
  }

  try {
    const res = await fetch("https://slack.com/api/chat.postMessage", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        channel: "#general",
        text: `*Agent Result Received*\n\`\`\`${JSON.stringify(data, null, 2)}\`\`\``,
      }),
    });
    const body = await res.json();
    if (body.ok) {
      console.log("[INTEGRATION:SLACK] Posted to Slack!");
    } else {
      console.error("[INTEGRATION:SLACK] Slack said no:", body.error);
    }
  } catch (err) {
    console.error(`[INTEGRATION:SLACK] Couldn't reach Slack: ${err.message}`);
  }
}

async function updateGoogleSheetRow(data, machineId) {
  const token = await getAccessToken(machineId, "google");
  if (!token) {
    console.log("[INTEGRATION:GOOGLE_SHEETS] No Google connection found — skipping");
    return;
  }

  try {
    const spreadsheetId = process.env.GOOGLE_SHEET_ID;
    if (!spreadsheetId) {
      console.log("[INTEGRATION:GOOGLE_SHEETS] Missing GOOGLE_SHEET_ID environment variable — skipping");
      return;
    }
    const res = await fetch(`https://sheets.googleapis.com/v4/spreadsheets/${encodeURIComponent(spreadsheetId)}/values/A1:append?valueInputOption=USER_ENTERED`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
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
    console.log("[INTEGRATION:GOOGLE_SHEETS] Row appended:", body.spreadsheetId ? "Nice, it worked!" : "Hmm, it didn't work");
  } catch (err) {
    console.error(`[INTEGRATION:GOOGLE_SHEETS] Couldn't write to sheet: ${err.message}`);
  }
}

async function createGithubIssue(data, machineId) {
  const token = await getAccessToken(machineId, "github", false);
  if (!token) {
    console.log("[INTEGRATION:GITHUB] No GitHub connection found — skipping");
    return;
  }

  try {
    const res = await fetch("https://api.github.com/repos/mutexflow/agent-results/issues", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        title: `Agent Result - ${data.workflowType || "general"} - ${new Date().toISOString().split("T")[0]}`,
        body: `## Agent Output\n\n**Workflow:** ${data.workflowType || "N/A"}\n**Complexity:** ${data.taskComplexity || "N/A"}\n**Prompt:** ${data.prompt || "N/A"}\n\n### Result\n\`\`\`\n${data.result || "No result"}\n\`\`\``,
        labels: ["ai-agent", "automated"],
      }),
    });
    const body = await res.json();
    console.log("[INTEGRATION:GITHUB] Issue created:", body.id ? "Done!" : "Didn't work");
  } catch (err) {
    console.error(`[INTEGRATION:GITHUB] Couldn't create GitHub issue: ${err.message}`);
  }
}

const integrationHandlers = {
  slack: async (data, machineId) => {
    await sendSlackNotification(data, machineId);
  },
  gmail: async (data, machineId) => {
    const token = await getAccessToken(machineId, "google");
    if (!token) {
      console.log("[INTEGRATION:GMAIL] No Google tokens found for machine — skipping");
      return;
    }
    console.log("[INTEGRATION:GMAIL] Gmail dispatch not yet implemented");
  },
  google_sheets: async (data, machineId) => {
    await updateGoogleSheetRow(data, machineId);
  },
  google: async (data, machineId) => {
    await updateGoogleSheetRow(data, machineId);
  },
  github: async (data, machineId) => {
    await createGithubIssue(data, machineId);
  },
  notion: async (data, machineId) => {
    console.log("[INTEGRATION:NOTION] Notion dispatch not yet implemented");
  },
  confluence: async (data, machineId) => {
    console.log("[INTEGRATION:CONFLUENCE] Confluence dispatch not yet implemented");
  },
  jira: async (data, machineId) => {
    console.log("[INTEGRATION:JIRA] Jira dispatch not yet implemented");
  },
  linear: async (data, machineId) => {
    console.log("[INTEGRATION:LINEAR] Linear dispatch not yet implemented");
  },
  salesforce: async (data, machineId) => {
    console.log("[INTEGRATION:SALESFORCE] Salesforce dispatch not yet implemented");
  },
  hubspot: async (data, machineId) => {
    console.log("[INTEGRATION:HUBSPOT] HubSpot dispatch not yet implemented");
  },
};

async function processIntegrations(resultData, targetIntegrations, machineId) {
  if (!targetIntegrations || targetIntegrations.length === 0) {
    console.log("[INTEGRATIONS] No apps selected — nothing to push to.");
    return;
  }

  if (!machineId) {
    console.log("[INTEGRATIONS] Can't find this machine — skipping integrations");
    return;
  }

  console.log("[INTEGRATIONS] Pushing results to:", targetIntegrations.join(", "));

  for (const integration of targetIntegrations) {
    if (integrationHandlers[integration]) {
      try {
        await integrationHandlers[integration](resultData, machineId);
      } catch (err) {
        console.error(`[INTEGRATIONS] Something broke while talking to ${integration}: ${err.message}`);
      }
    } else {
      console.log(`[INTEGRATIONS] Don't know how to push to: ${integration}`);
    }
  }

  console.log("[INTEGRATIONS] All done with integrations.");
}

module.exports = { processIntegrations };
