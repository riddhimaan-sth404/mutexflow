async function sendSlackNotification(data) {
  const url = process.env.SLACK_WEBHOOK_URL;
  if (!url) {
    console.log("[INTEGRATION:SLACK] No SLACK_WEBHOOK_URL configured — skipping");
    return;
  }

  try {
    const payload = { text: `*Agent Result Received*\n\`\`\`${JSON.stringify(data, null, 2)}\`\`\`` };
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      console.error(`[INTEGRATION:SLACK] Webhook returned ${res.status} ${res.statusText}`);
      return;
    }
    console.log("[INTEGRATION:SLACK] Notification delivered successfully.");
  } catch (err) {
    console.error(`[INTEGRATION:SLACK] Request failed: ${err.message}`);
  }
}

async function updateGoogleSheetRow(data) {
  const url = process.env.SHEETS_WEBHOOK_URL;
  if (!url) {
    console.log("[INTEGRATION:GOOGLE_SHEETS] No SHEETS_WEBHOOK_URL configured — skipping");
    return;
  }

  try {
    const payload = {
      workflowType: data.workflowType || "N/A",
      taskComplexity: data.taskComplexity || "N/A",
      prompt: data.prompt || "N/A",
      result: data.result || "N/A",
      timestamp: new Date().toISOString(),
    };
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      console.error(`[INTEGRATION:GOOGLE_SHEETS] Webhook returned ${res.status} ${res.statusText}`);
      return;
    }
    console.log("[INTEGRATION:GOOGLE_SHEETS] Row appended successfully.");
  } catch (err) {
    console.error(`[INTEGRATION:GOOGLE_SHEETS] Request failed: ${err.message}`);
  }
}

async function createGithubIssue(data) {
  const url = process.env.GITHUB_WEBHOOK_URL;
  if (!url) {
    console.log("[INTEGRATION:GITHUB] No GITHUB_WEBHOOK_URL configured — skipping");
    return;
  }

  try {
    const payload = {
      workflowType: data.workflowType || "N/A",
      taskComplexity: data.taskComplexity || "N/A",
      prompt: data.prompt || "N/A",
      result: data.result || "N/A",
      timestamp: new Date().toISOString(),
    };
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      console.error(`[INTEGRATION:GITHUB] Webhook returned ${res.status} ${res.statusText}`);
      return;
    }
    console.log("[INTEGRATION:GITHUB] Issue created successfully.");
  } catch (err) {
    console.error(`[INTEGRATION:GITHUB] Request failed: ${err.message}`);
  }
}

async function processIntegrations(resultData, targetIntegrations) {
  if (!targetIntegrations || targetIntegrations.length === 0) {
    console.log("[INTEGRATIONS] No target integrations configured.");
    return;
  }

  console.log("[INTEGRATIONS] Processing integrations for:", targetIntegrations.join(", "));

  for (const integration of targetIntegrations) {
    try {
      switch (integration) {
        case "slack":
          sendSlackNotification(resultData);
          break;
        case "google_sheets":
          updateGoogleSheetRow(resultData);
          break;
        case "github":
          createGithubIssue(resultData);
          break;
        default:
          console.log(`[INTEGRATIONS] Unknown integration target: ${integration}`);
      }
    } catch (err) {
      console.error(`[INTEGRATIONS] Error processing ${integration}:`, err.message);
    }
  }

  console.log("[INTEGRATIONS] All target integrations processed.");
}

module.exports = { processIntegrations };
