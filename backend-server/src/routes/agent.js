const { Router } = require("express");
const { validateSubscription } = require("../middleware/validateSubscription");
const { processIntegrations } = require("../services/integrationService");
const { AGENT_PROMPTS } = require("../services/agentPrompts");
const { CLOUD_CASCADES } = require("../config/aiProviders");

const router = Router();

router.post("/agent/process-standard", validateSubscription, async (req, res) => {
  try {
    const { prompt, taskComplexity, workflowType, targetIntegrations } = req.body;

    if (!prompt || typeof prompt !== "string") {
      return res.status(400).json({ ok: false, error: "I need a prompt to work with" });
    }

    const entry = AGENT_PROMPTS[workflowType] || AGENT_PROMPTS.general_reasoning;
    const messages = [
      { role: "system", content: entry.system },
      { role: "user", content: prompt },
    ];

    let result = null;
    for (const provider of CLOUD_CASCADES) {
      try {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 5000);

        const response = await fetch(provider.url, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            "Authorization": `Bearer ${provider.key}`,
          },
          body: JSON.stringify({ model: provider.model, messages }),
          signal: controller.signal,
        });

        clearTimeout(timeout);

        if (!response.ok) {
          console.warn(`[Cascade] ${provider.name} said no (HTTP ${response.status}) — trying the next one...`);
          continue;
        }

        const data = await response.json();
        result = data.choices?.[0]?.message?.content || data;
        break;
      } catch (err) {
        console.warn(`[Cascade] ${provider.name} didn't respond (${err.message}) — on to the next...`);
        continue;
      }
    }

    if (!result) {
      return res.status(502).json({ ok: false, error: "None of the cloud providers could handle this right now" });
    }

    processIntegrations(
      { result, workflowType, taskComplexity, prompt },
      targetIntegrations,
      req.machineId
    );

    return res.json({ ok: true, result });
  } catch (err) {
    console.error("[CASCADE] Something unexpected:", err);
    return res.status(500).json({ ok: false, error: "Something went wrong on our end" });
  }
});

router.post("/integrations/dispatch", validateSubscription, async (req, res) => {
  try {
    const { result, workflowType, taskComplexity, prompt, targetIntegrations } = req.body;

    processIntegrations(
      { result, workflowType, taskComplexity, prompt },
      targetIntegrations,
      req.machineId
    );

    return res.json({ ok: true });
  } catch (err) {
    console.error("[DISPATCH] Something unexpected:", err);
    return res.status(500).json({ ok: false, error: "Something went wrong on our end" });
  }
});

module.exports = router;
