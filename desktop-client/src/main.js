const {
  app,
  BrowserWindow,
  ipcMain,
  safeStorage,
  shell,
} = require("electron");
const os = require("os");
const path = require("path");
const fs = require("fs");
const { spawn } = require("child_process");
const { machineIdSync } = require("node-machine-id");
const { BACKEND_URL } = require("./config");

const isEncryptionAvailable = safeStorage.isEncryptionAvailable();
const backendUrl = BACKEND_URL;

const modelsDir = path.join(app.getPath("userData"), "models");
const oauthSignalDir = path.join(os.tmpdir(), "mutexflow-oauth");

if (!fs.existsSync(modelsDir)) {
  fs.mkdirSync(modelsDir, { recursive: true });
}
if (!fs.existsSync(oauthSignalDir)) {
  fs.mkdirSync(oauthSignalDir, { recursive: true });
}

const llamaBinary = process.platform === "win32" ? "llama-server.exe" : "llama-server";
const llamaCppPath = path.join(process.resourcesPath || path.join(__dirname, ".."), "resources", "bin", llamaBinary);

let hardwareFingerprint = "UNKNOWN_HWID";
try {
  hardwareFingerprint = machineIdSync({ original: true });
} catch (err) {
  console.error("[SYSTEM] Failed to read HWID:", err.message);
}

let llamaProcess = null;
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

function killLlamaProcess() {
  if (llamaProcess) {
    try {
      llamaProcess.kill("SIGTERM");
      console.log("[llama.cpp] Process terminated.");
    } catch (err) {
      console.error("[llama.cpp] Kill error:", err.message);
    }
    llamaProcess = null;
  }
}

async function waitForLlamaReady(timeoutMs) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const resp = await fetch("http://localhost:8080/v1/models", {
        signal: AbortSignal.timeout(2000),
      });
      if (resp.ok) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 1000));
  }
  return false;
}

ipcMain.handle("auth:verify-license", async (_event, { licenseKey }) => {
  if (!licenseKey) {
    return { ok: false, error: "License key is required" };
  }

  try {
    const response = await fetch(`${backendUrl}/api/v1/auth/verify-license`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ licenseKey, machineId: hardwareFingerprint }),
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

    if (!response.ok) {
      const text = await response.text().catch(() => "");
      return { ok: false, error: `Backend returned HTTP ${response.status}${text ? ": " + text.slice(0, 200) : ""}` };
    }

    const data = await response.json();
    return data;
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "Request timed out after 120s" };
    }
    return { ok: false, error: `Connection dropped: ${err.message}` };
  }
});

ipcMain.handle("download-gguf-model", async (_event, { repo, file }) => {
  const url = `https://huggingface.co/${repo}/resolve/main/${file}`;
  const destPath = path.join(modelsDir, file);

  try {
    if (fs.existsSync(destPath)) {
      return { ok: true, message: "Model already downloaded" };
    }

    const response = await fetch(url);
    if (!response.ok) {
      return { ok: false, error: `Hugging Face returned HTTP ${response.status}` };
    }

    const total = parseInt(response.headers.get("content-length") || "0", 10);
    const reader = response.body.getReader();
    const writeStream = fs.createWriteStream(destPath);
    let downloaded = 0;

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        downloaded += value.length;
        writeStream.write(Buffer.from(value));
        if (mainWindow && !mainWindow.isDestroyed()) {
          mainWindow.webContents.send("download-progress", {
            bytes: downloaded,
            total,
            filename: file,
          });
        }
      }
    } finally {
      await new Promise((resolve) => writeStream.end(resolve));
    }

    return { ok: true };
  } catch (err) {
    if (fs.existsSync(destPath)) {
      try { fs.unlinkSync(destPath); } catch {}
    }
    return { ok: false, error: `Download failed: ${err.message}` };
  }
});

ipcMain.handle("spawn-llama-cpp", async (_event, { modelFile }) => {
  if (llamaProcess) {
    return { ok: true };
  }

  const alreadyRunning = await waitForLlamaReady(3000);
  if (alreadyRunning) {
    return { ok: true };
  }

  if (!fs.existsSync(llamaCppPath)) {
    return { ok: false, error: `llama server binary not found at ${llamaCppPath}. Place it in resources/bin/.` };
  }

  if (!modelFile) {
    return { ok: false, error: "No model file selected. Select a model in Local AI Hub first." };
  }

  const modelPath = path.join(modelsDir, modelFile);
  if (!fs.existsSync(modelPath)) {
    return { ok: false, error: `Model file not found at ${modelPath}. Download it first via Local AI Hub.` };
  }

  const args = [
    "--model", modelPath,
    "--port", "8080",
    "--ctx-size", "4096",
    "--threads", "4",
  ];

  try {
    llamaProcess = spawn(llamaCppPath, args, {
      stdio: ["ignore", "pipe", "pipe"],
    });

    llamaProcess.stdout.on("data", (data) => {
      console.log(`[llama.cpp:stdout] ${data.toString().trim()}`);
    });

    llamaProcess.stderr.on("data", (data) => {
      console.log(`[llama.cpp:stderr] ${data.toString().trim()}`);
    });

    llamaProcess.on("error", (err) => {
      console.error("[llama.cpp] Spawn error:", err.message);
      llamaProcess = null;
    });

    llamaProcess.on("exit", (code) => {
      console.log(`[llama.cpp] Process exited with code ${code}`);
      llamaProcess = null;
    });

    const ready = await waitForLlamaReady(30000);
    if (!ready) {
      killLlamaProcess();
      return { ok: false, error: "llama.cpp failed to start within 30s. Check the model path and logs." };
    }

    return { ok: true };
  } catch (err) {
    killLlamaProcess();
    return { ok: false, error: `Failed to spawn llama.cpp: ${err.message}` };
  }
});

ipcMain.handle("integrations:dispatch", async (_event, { result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations }) => {
  try {
    const response = await fetch(`${backendUrl}/api/v1/integrations/dispatch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations, machineId: hardwareFingerprint }),
    });

    const data = await response.json();
    return data;
  } catch (err) {
    return { ok: false, error: `Integration dispatch failed: ${err.message}` };
  }
});

ipcMain.handle("oauth:connect", async (_event, { provider }) => {
  const authUrl = `${backendUrl}/api/v1/auth/${provider}/connect?machineId=${encodeURIComponent(hardwareFingerprint)}`;
  const signalFile = path.join(oauthSignalDir, `${hardwareFingerprint}_${provider}`);

  shell.openExternal(authUrl);

  for (let i = 0; i < 120; i++) {
    await new Promise(r => setTimeout(r, 1000));
    if (fs.existsSync(signalFile)) {
      try { fs.unlinkSync(signalFile); } catch {}
      return { ok: true, provider };
    }
  }

  return { ok: false, error: "OAuth timed out after 120s" };
});

app.whenReady().then(createWindow);

app.on("window-all-closed", () => {
  killLlamaProcess();
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
  killLlamaProcess();
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});
