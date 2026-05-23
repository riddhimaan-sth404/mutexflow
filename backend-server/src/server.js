require("dotenv").config();

const querystring = require("querystring");
const fs = require("fs");
const path = require("path");
const os = require("os");
const express = require("express");
const cors = require("cors");
const axios = require("axios");
const { createClient } = require("@supabase/supabase-js");
const agentRouter = require("./routes/agent");

const PORT = process.env.PORT || 3001;
const redirectPort = PORT;

const app = express();

app.use(cors());
app.use(express.json({ limit: "1mb" }));
app.use("/api/v1", agentRouter);

const supabaseUrl = process.env.SUPABASE_URL;
const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

const supabase = supabaseUrl && supabaseKey ? createClient(supabaseUrl, supabaseKey) : null;

const oauthSignalDir = path.join(os.tmpdir(), "mutexflow-oauth");
if (!fs.existsSync(oauthSignalDir)) {
  fs.mkdirSync(oauthSignalDir, { recursive: true });
}

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
    scope: "https://www.googleapis.com/auth/spreadsheets https://www.googleapis.com/auth/drive https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/calendar.events",
    extraParams: "&response_type=code&access_type=offline&prompt=consent",
  },
  microsoft: {
    authorizeUrl: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    tokenUrl: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    clientId: process.env.MICROSOFT_CLIENT_ID,
    clientSecret: process.env.MICROSOFT_CLIENT_SECRET,
    scope: "offline_access Mail.ReadWrite Calendars.ReadWrite Files.ReadWrite.All",
    extraParams: "&response_mode=query",
  },
  hubspot: {
    authorizeUrl: "https://app.hubspot.com/oauth/authorize",
    tokenUrl: "https://api.hubapi.com/oauth/v1/token",
    clientId: process.env.HUBSPOT_CLIENT_ID,
    clientSecret: process.env.HUBSPOT_CLIENT_SECRET,
    scope: "crm.objects.contacts.read crm.objects.companies.read",
  },
  salesforce: {
    authorizeUrl: "https://login.salesforce.com/services/oauth2/authorize",
    tokenUrl: "https://login.salesforce.com/services/oauth2/token",
    clientId: process.env.SALESFORCE_CLIENT_ID,
    clientSecret: process.env.SALESFORCE_CLIENT_SECRET,
    scope: "api id openid",
    extraParams: "&response_type=code",
  },
  jira: {
    authorizeUrl: "https://auth.atlassian.com/authorize",
    tokenUrl: "https://auth.atlassian.com/oauth/token",
    clientId: process.env.JIRA_CLIENT_ID,
    clientSecret: process.env.JIRA_CLIENT_SECRET,
    scope: "read:jira-work write:jira-work offline_access",
    extraParams: "&audience=api.atlassian.com&prompt=consent",
  },
  linear: {
    authorizeUrl: "https://linear.app/oauth/authorize",
    tokenUrl: "https://api.linear.app/oauth/token",
    clientId: process.env.LINEAR_CLIENT_ID,
    clientSecret: process.env.LINEAR_CLIENT_SECRET,
    scope: "read write",
  },
  notion: {
    authorizeUrl: "https://api.notion.com/v1/oauth/authorize",
    tokenUrl: "https://api.notion.com/v1/oauth/token",
    clientId: process.env.NOTION_CLIENT_ID,
    clientSecret: process.env.NOTION_CLIENT_SECRET,
    scope: "",
  },
  confluence: {
    authorizeUrl: "https://auth.atlassian.com/authorize",
    tokenUrl: "https://auth.atlassian.com/oauth/token",
    clientId: process.env.CONFLUENCE_CLIENT_ID || process.env.JIRA_CLIENT_ID,
    clientSecret: process.env.CONFLUENCE_CLIENT_SECRET || process.env.JIRA_CLIENT_SECRET,
    scope: "read:confluence-content write:confluence-content offline_access",
    extraParams: "&audience=api.atlassian.com&prompt=consent",
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

  if (!config.clientId) {
    return res.status(501).send(`OAuth not configured for ${provider}: missing client_id. Set ${provider.toUpperCase()}_CLIENT_ID in .env`);
  }

  const redirectUri = `http://localhost:${redirectPort}/api/v1/auth/${provider}/callback`;
  const url =
    `${config.authorizeUrl}?` +
    `client_id=${encodeURIComponent(config.clientId)}&` +
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
    const tokenParams = {
      client_id: config.clientId,
      client_secret: config.clientSecret,
      code,
      grant_type: "authorization_code",
      redirect_uri: `http://localhost:${redirectPort}/api/v1/auth/${provider}/callback`,
    };

    let tokenResponse;
    if (provider === "github") {
      tokenResponse = await axios.post(config.tokenUrl, tokenParams, {
        headers: { Accept: "application/json", "Content-Type": "application/json" },
      });
    } else {
      tokenResponse = await axios.post(config.tokenUrl, querystring.stringify(tokenParams), {
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
      });
    }

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

    try {
      const signalFile = path.join(oauthSignalDir, `${machineId}_${provider}`);
      fs.writeFileSync(signalFile, Date.now().toString());
    } catch (err) {
      console.error(`[OAUTH:${provider}] Failed to write signal file:`, err.message);
    }

    res.send(`
      <!DOCTYPE html>
      <html lang="en">
      <head><meta charset="UTF-8"><title>MutexFlow - Connected</title></head>
      <body style="font-family: system-ui; text-align: center; padding: 3rem;">
        <h1>✅ ${provider} Connected</h1>
        <p>You can close this tab and return to MutexFlow.</p>
      </body>
      </html>
    `);
  } catch (err) {
    const detail = err.response?.data ? JSON.stringify(err.response.data) : err.message;
    console.error(`[OAUTH:${provider}] Callback error: ${detail}`);
    res.status(500).send(`OAuth callback failed for provider: ${provider}`);
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

app.listen(PORT, () => {
  console.log(`[backend] B2B AI Proxy running on port ${PORT}`);
});
