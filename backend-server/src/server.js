require("dotenv").config();

const token = process.env.PUTER_AUTH_TOKEN;
if (!token) {
  console.error("[FATAL] PUTER_AUTH_TOKEN is required");
  process.exit(1);
}

require("@heyputer/puter.js/src/init.cjs").init(token);

const express = require("express");
const cors = require("cors");
const { createClient } = require("@supabase/supabase-js");
const agentRouter = require("./routes/agent");

const app = express();

app.use(cors());
app.use(express.json({ limit: "1mb" }));
app.use("/api/v1", agentRouter);

app.post("/api/v1/auth/verify-license", async (req, res) => {
  const { licenseKey } = req.body;

  if (!licenseKey || typeof licenseKey !== "string") {
    return res.status(403).json({ ok: false, error: "Missing or invalid license key" });
  }

  const supabaseUrl = process.env.SUPABASE_URL;
  const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

  if (!supabaseUrl || !supabaseKey) {
    return res.json({ ok: true });
  }

  try {
    const supabase = createClient(supabaseUrl, supabaseKey);
    const { data, error } = await supabase
      .from("licenses")
      .select("id")
      .eq("key", licenseKey)
      .eq("status", "active")
      .maybeSingle();

    if (error || !data) {
      return res.status(403).json({ ok: false, error: "Invalid or inactive license key" });
    }

    return res.json({ ok: true });
  } catch (err) {
    console.error("[VERIFY-LICENSE]", err.message);
    return res.status(500).json({ ok: false, error: "Internal server error" });
  }
});

const PORT = process.env.PORT || 3001;
app.listen(PORT, () => {
  console.log(`[backend] B2B AI Proxy running on port ${PORT}`);
});
