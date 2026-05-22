const {
  app,
  BrowserWindow,
  ipcMain,
  safeStorage,
} = require("electron");
const path = require("path");
const { BACKEND_URL } = require("./config");

const NANGO_HOST = process.env.NANGO_HOST || "http://localhost:3003";

const isEncryptionAvailable = safeStorage.isEncryptionAvailable();
let backendUrl = BACKEND_URL;

if (isEncryptionAvailable) {
  const encrypted = safeStorage.encryptString(BACKEND_URL);
  backendUrl = safeStorage.decryptString(encrypted);
}

let mainWindow;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      nativeWindowOpen: true,
      preload: path.join(__dirname, "preload.js"),
    },
  });

  mainWindow.loadFile(path.join(__dirname, "interface", "index.html"));
}

ipcMain.handle("auth:verify-license", async (_event, { licenseKey }) => {
  if (!licenseKey) {
    return { ok: false, error: "License key is required" };
  }

  try {
    const response = await fetch(`${backendUrl}/api/v1/auth/verify-license`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ licenseKey }),
    });

    const data = await response.json();
    return data;
  } catch (err) {
    return { ok: false, error: `License verification failed: ${err.message}` };
  }
});

ipcMain.handle("agent:run-local", async (_event, { prompt, taskComplexity, workflowType }) => {
  try {
    const messages = [
      { role: "system", content: `You are MutexFlow, a helpful enterprise assistant running on local infrastructure. Workflow: ${workflowType}, Complexity: ${taskComplexity}. Provide clear, concise responses.` },
      { role: "user", content: prompt },
    ];

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 60000);

    const response = await fetch("http://localhost:8080/v1/chat/completions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: "local-model", messages }),
      signal: controller.signal,
    });

    clearTimeout(timeout);

    if (!response.ok) {
      return { ok: false, error: `llama.cpp returned HTTP ${response.status}` };
    }

    const data = await response.json();
    const content = data.choices?.[0]?.message?.content;
    if (!content) {
      return { ok: false, error: "Empty response from local model" };
    }

    return { ok: true, result: content };
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "Local Core request timed out after 60s" };
    }
    return { ok: false, error: `Local Core connection failed: ${err.message}` };
  }
});

ipcMain.handle("agent:run-byok", async (_event, { prompt, taskComplexity, workflowType, byokKey }) => {
  if (!byokKey) {
    return { ok: false, error: "OpenRouter key is required for BYOK mode" };
  }

  try {
    const messages = [
      { role: "system", content: `You are MutexFlow, a helpful enterprise assistant using OpenRouter. Workflow: ${workflowType}, Complexity: ${taskComplexity}. Provide clear, concise responses.` },
      { role: "user", content: prompt },
    ];

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 120000);

    const response = await fetch("https://openrouter.ai/api/v1/chat/completions", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${byokKey}`,
      },
      body: JSON.stringify({ model: "openai/gpt-4o-mini", messages }),
      signal: controller.signal,
    });

    clearTimeout(timeout);

    if (!response.ok) {
      const errBody = await response.text().catch(() => "");
      return { ok: false, error: `OpenRouter returned HTTP ${response.status}: ${errBody}` };
    }

    const data = await response.json();
    const content = data.choices?.[0]?.message?.content;
    if (!content) {
      return { ok: false, error: "Empty response from OpenRouter" };
    }

    return { ok: true, result: content };
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "BYOK request timed out after 120s" };
    }
    return { ok: false, error: `BYOK request failed: ${err.message}` };
  }
});

ipcMain.handle("agent:run-standard", async (_event, { prompt, licenseKey, taskComplexity, workflowType, targetIntegrations }) => {
  if (!prompt || !licenseKey) {
    return { ok: false, error: "Prompt and license key are required" };
  }

  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 120000);

    const response = await fetch(`${backendUrl}/api/v1/agent/process-standard`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ prompt, licenseKey, taskComplexity, workflowType, targetIntegrations }),
      signal: controller.signal,
    });

    clearTimeout(timeout);

    const data = await response.json();
    return data;
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "Request timed out after 120s" };
    }
    return { ok: false, error: `Connection dropped: ${err.message}` };
  }
});

ipcMain.handle("integrations:dispatch", async (_event, { result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations }) => {
  try {
    const response = await fetch(`${backendUrl}/api/v1/integrations/dispatch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations }),
    });

    const data = await response.json();
    return data;
  } catch (err) {
    return { ok: false, error: `Integration dispatch failed: ${err.message}` };
  }
});

ipcMain.handle("nango:auth", async (_event, { provider, connectionId }) => {
  return new Promise((resolve) => {
    const authUrl = `${NANGO_HOST}/oauth/auth?provider_config_key=${provider}&connection_id=${connectionId}`;

    const authWindow = new BrowserWindow({
      width: 800,
      height: 700,
      parent: mainWindow,
      modal: true,
      webPreferences: {
        nodeIntegration: false,
        contextIsolation: true,
      },
    });

    authWindow.loadURL(authUrl);

    const filter = { urls: [`${NANGO_HOST}/oauth/callback*`] };
    authWindow.webContents.on("will-redirect", (_event, url) => {
      if (url.startsWith(`${NANGO_HOST}/oauth/callback`)) {
        authWindow.close();
        resolve({ ok: true, provider, connectionId });
      }
    });

    authWindow.on("closed", () => {
      resolve({ ok: false, error: "Auth window closed by user" });
    });
  });
});

app.whenReady().then(createWindow);

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});
