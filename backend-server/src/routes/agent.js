const { Router } = require("express");
const { validateSubscription } = require("../middleware/validateSubscription");
const { runAi } = require("../services/puterAi");

const router = Router();

router.post("/agent/run", async (req, res) => {
  try {
    const { prompt, licenseKey, taskComplexity, workflowType } = req.body;

    if (!prompt || typeof prompt !== "string") {
      return res.status(400).json({ ok: false, error: "Missing or invalid 'prompt'" });
    }

    if (!validateSubscription(licenseKey)) {
      return res.status(403).json({ ok: false, error: "Invalid or missing license key" });
    }

    const result = await runAi(prompt, taskComplexity, workflowType);

    if (!result.success) {
      return res.status(502).json({ ok: false, error: result.error });
    }

    return res.json({ ok: true, result: result.data });
  } catch (err) {
    console.error("[UNHANDLED]", err);
    return res.status(500).json({ ok: false, error: "Internal server error" });
  }
});

module.exports = router;
