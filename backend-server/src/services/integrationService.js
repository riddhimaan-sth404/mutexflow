const { Nango } = require("@nangohq/node");

const nangoSecretKey = process.env.NANGO_SECRET_KEY;
const nangoHost = process.env.NANGO_HOST;

let nango = null;
if (nangoSecretKey && nangoHost) {
  nango = new Nango({ secretKey: nangoSecretKey, host: nangoHost });
}

async function sendSlackNotification(data, connectionId) {
  if (!nango) {
    console.log("[INTEGRATION:SLACK] Nango not configured — skipping");
    return;
  }

  try {
    const res = await nango.proxy({
      connectionId,
      providerConfigKey: "slack",
      endpoint: "/chat.postMessage",
      method: "POST",
      data: {
        channel: "#general",
        text: `*Agent Result Received*\n\`\`\`${JSON.stringify(data, null, 2)}\`\`\``,
      },
    });
    console.log("[INTEGRATION:SLACK] Notification delivered:", res.status);
  } catch (err) {
    console.error(`[INTEGRATION:SLACK] Nango proxy failed: ${err.message}`);
  }
}

async function updateGoogleSheetRow(data, connectionId) {
  if (!nango) {
    console.log("[INTEGRATION:GOOGLE_SHEETS] Nango not configured — skipping");
    return;
  }

  try {
    const res = await nango.proxy({
      connectionId,
      providerConfigKey: "google-sheets",
      endpoint: "/v4/spreadsheets/Sheet1",
      method: "POST",
      data: {
        workflowType: data.workflowType || "N/A",
        taskComplexity: data.taskComplexity || "N/A",
        prompt: data.prompt || "N/A",
        result: data.result || "N/A",
        timestamp: new Date().toISOString(),
      },
    });
    console.log("[INTEGRATION:GOOGLE_SHEETS] Row appended:", res.status);
  } catch (err) {
    console.error(`[INTEGRATION:GOOGLE_SHEETS] Nango proxy failed: ${err.message}`);
  }
}

async function createGithubIssue(data, connectionId) {
  if (!nango) {
    console.log("[INTEGRATION:GITHUB] Nango not configured — skipping");
    return;
  }

  try {
    const res = await nango.proxy({
      connectionId,
      providerConfigKey: "github",
      endpoint: "/repos/mutexflow/agent-results/issues",
      method: "POST",
      data: {
        title: `Agent Result - ${data.workflowType || "general"} - ${new Date().toISOString().split("T")[0]}`,
        body: `## Agent Output\n\n**Workflow:** ${data.workflowType || "N/A"}\n**Complexity:** ${data.taskComplexity || "N/A"}\n**Prompt:** ${data.prompt || "N/A"}\n\n### Result\n\`\`\`\n${data.result || "No result"}\n\`\`\``,
        labels: ["ai-agent", "automated"],
      },
    });
    console.log("[INTEGRATION:GITHUB] Issue created:", res.status);
  } catch (err) {
    console.error(`[INTEGRATION:GITHUB] Nango proxy failed: ${err.message}`);
  }
}

async function processIntegrations(resultData, targetIntegrations, nangoConnectionId) {
  if (!targetIntegrations || targetIntegrations.length === 0) {
    console.log("[INTEGRATIONS] No target integrations configured.");
    return;
  }

  if (!nangoConnectionId) {
    console.log("[INTEGRATIONS] No Nango connection ID available — skipping integrations");
    return;
  }

  console.log("[INTEGRATIONS] Processing integrations for:", targetIntegrations.join(", "));

  for (const integration of targetIntegrations) {
    try {
      switch (integration) {
        case "slack":
          await sendSlackNotification(resultData, nangoConnectionId);
          break;
        case "google_sheets":
          await updateGoogleSheetRow(resultData, nangoConnectionId);
          break;
        case "github":
          await createGithubIssue(resultData, nangoConnectionId);
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
