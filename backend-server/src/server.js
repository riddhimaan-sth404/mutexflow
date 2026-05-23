require("dotenv").config();

const express = require("express");
const cors = require("cors");
const axios = require("axios");
const { createClient } = require("@supabase/supabase-js");
const agentRouter = require("./routes/agent");

const app = express();

app.use(cors());
app.use(express.json({ limit: "1mb" }));
app.use("/api/v1", agentRouter);

const supabaseUrl = process.env.SUPABASE_URL;
const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

const supabase = supabaseUrl && supabaseKey ? createClient(supabaseUrl, supabaseKey) : null;

const OAUTH_CONFIG = {
  slack: {
    authorizeUrl: "https://slack.com/oauth/v2/authorize",
    tokenUrl: "https://slack.com/api/oauth.v2.access",
    clientId: process.env.SLACK_CLIENT_ID,
    clientSecret: process.env.SLACK_CLIENT_SECRET,
    scope: "chat:write,commands",
  },
  github: {
    authorizeUrl: "https://github.com/login/oauth/authorize",
    tokenUrl: "https://github.com/login/oauth/access_token",
    clientId: process.env.GITHUB_CLIENT_ID,
    clientSecret: process.env.GITHUB_CLIENT_SECRET,
    scope: "repo,user",
  },
  google: {
    authorizeUrl: "https://accounts.google.com/o/oauth2/v2/auth",
    tokenUrl: "https://oauth2.googleapis.com/token",
    clientId: process.env.GOOGLE_CLIENT_ID,
    clientSecret: process.env.GOOGLE_CLIENT_SECRET,
    scope: "https://www.googleapis.com/auth/spreadsheets https://www.googleapis.com/auth/drive",
    extraParams: "&response_type=code&access_type=offline&prompt=consent",
  },
};

app.get("/api/v1/auth/:provider/connect", (req, res) => {
  const { provider } = req.params;
  const { machineId } = req.query;

  const config = OAUTH_CONFIG[provider];
  if (!config) {
    return res.status(400).send(`Unknown provider: ${provider}`);
  }

  if (!machineId) {
    return res.status(400).send("Missing machineId query parameter");
  }

  const redirectUri = `http://localhost:3001/api/v1/auth/${provider}/callback`;
  const url =
    `${config.authorizeUrl}?` +
    `client_id=${config.clientId}&` +
    `scope=${encodeURIComponent(config.scope)}&` +
    `redirect_uri=${encodeURIComponent(redirectUri)}&` +
    `state=${encodeURIComponent(machineId)}` +
    (config.extraParams || "");

  res.redirect(url);
});

app.get("/api/v1/auth/:provider/callback", async (req, res) => {
  const { provider } = req.params;
  const { code, state: machineId } = req.query;

  const config = OAUTH_CONFIG[provider];
  if (!config) {
    return res.status(400).send(`Unknown provider: ${provider}`);
  }

  if (!code) {
    return res.status(400).send("Missing authorization code");
  }

  if (!machineId) {
    return res.status(400).send("Missing state parameter");
  }

  try {
    const tokenResponse = await axios.post(
      config.tokenUrl,
      {
        client_id: config.clientId,
        client_secret: config.clientSecret,
        code,
        redirect_uri: `http://localhost:3001/api/v1/auth/${provider}/callback`,
      },
      {
        headers: provider === "github"
          ? { Accept: "application/json" }
          : { "Content-Type": "application/json" },
      }
    );

    const tokenData = tokenResponse.data;
    const accessToken = tokenData.access_token;
    const refreshToken = tokenData.refresh_token || null;

    if (!accessToken) {
      console.error(`[OAUTH:${provider}] Token exchange failed:`, tokenData);
      return res.status(502).send("Token exchange failed");
    }

    if (supabase) {
      const { error: upsertError } = await supabase
        .from("user_integrations")
        .upsert({
          machine_id: machineId,
          provider,
          access_token: accessToken,
          refresh_token: refreshToken,
          updated_at: new Date(),
        });

      if (upsertError) {
        console.error(`[OAUTH:${provider}] Failed to save tokens:`, upsertError.message);
      }
    }

    res.send(`
      <script>
        alert("${provider} successfully integrated with your MutexFlow Agent!");
        window.close();
      </script>
    `);
  } catch (err) {
    console.error(`[OAUTH:${provider}] Callback error:`, err.message);
    res.status(500).send("OAuth callback failed");
  }
});

app.post("/api/v1/auth/verify-license", async (req, res) => {
  const { licenseKey, machineId } = req.body;

  console.log(`[AUTH] Verifying License: ${licenseKey} for Machine: ${machineId}`);

  if (!licenseKey || typeof licenseKey !== "string") {
    return res.status(403).json({ ok: false, error: "Missing or invalid license key" });
  }

  if (!machineId || typeof machineId !== "string") {
    return res.status(403).json({ ok: false, error: "Machine fingerprint is required" });
  }

  if (!supabase) {
    return res.json({ ok: true });
  }

  try {
    const { data, error } = await supabase
      .from("licenses")
      .select("id, machine_id")
      .eq("key", licenseKey)
      .eq("status", "active")
      .maybeSingle();

    if (error || !data) {
      return res.status(403).json({ ok: false, error: "Invalid or inactive license key" });
    }

    if (!data.machine_id) {
      const { error: updateError } = await supabase
        .from("licenses")
        .update({ machine_id: machineId })
        .eq("id", data.id);

      if (updateError) {
        console.error("[VERIFY-LICENSE] Failed to bind machine:", updateError.message);
        return res.status(500).json({ ok: false, error: "Failed to bind license to machine" });
      }

      console.log(`[AUTH] License ${licenseKey} bound to machine ${machineId}`);
      return res.json({ ok: true });
    }

    if (data.machine_id !== machineId) {
      return res.status(403).json({ ok: false, error: "License violation: This key is already bound to another machine." });
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
