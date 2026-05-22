function sendSlackNotification(data) {
  console.log("[INTEGRATION:SLACK] Sending notification to Slack webhook...");
  console.log("[INTEGRATION:SLACK] Payload:", JSON.stringify({
    channel: "#ai-agent-results",
    username: "MutexFlow Agent",
    text: `*Agent Result Received*\n\`\`\`${JSON.stringify(data, null, 2)}\`\`\``,
    icon_emoji: ":robot_face:"
  }, null, 2));
  console.log("[INTEGRATION:SLACK] Notification delivered successfully.");
}

function updateGoogleSheetRow(data) {
  console.log("[INTEGRATION:GOOGLE_SHEETS] Appending row to Google Sheet...");
  console.log("[INTEGRATION:GOOGLE_SHEETS] Row data:", JSON.stringify({
    spreadsheetId: "mock_spreadsheet_id",
    range: "Sheet1!A:E",
    values: [[new Date().toISOString(), data.workflowType || "N/A", data.taskComplexity || "N/A", data.prompt || "N/A", data.result?.substring(0, 200) || "N/A"]]
  }, null, 2));
  console.log("[INTEGRATION:GOOGLE_SHEETS] Row appended successfully.");
}

function createGithubIssue(data) {
  console.log("[INTEGRATION:GITHUB] Creating GitHub issue...");
  console.log("[INTEGRATION:GITHUB] Issue payload:", JSON.stringify({
    owner: "mutexflow",
    repo: "agent-results",
    title: `Agent Result - ${data.workflowType || "general"} - ${new Date().toISOString().split("T")[0]}`,
    body: `## Agent Output\n\n**Workflow:** ${data.workflowType || "N/A"}\n**Complexity:** ${data.taskComplexity || "N/A"}\n**Prompt:** ${data.prompt || "N/A"}\n\n### Result\n\`\`\`\n${data.result || "No result"}\n\`\`\``,
    labels: ["ai-agent", "automated"]
  }, null, 2));
  console.log("[INTEGRATION:GITHUB] Issue created successfully.");
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
