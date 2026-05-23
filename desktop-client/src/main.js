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
  console.error("[SYSTEM] Couldn't read machine ID:", err.message);
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
      console.log("[llama.cpp] Local engine stopped.");
    } catch (err) {
      console.error("[llama.cpp] Couldn't stop the engine:", err.message);
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
    return { ok: false, error: "I need your license key to check" };
  }

  try {
    const response = await fetch(`${backendUrl}/api/v1/auth/verify-license`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ licenseKey, machineId: hardwareFingerprint }),
    });

    if (!response.ok) {
      const text = await response.text().catch(() => "");
      return { ok: false, error: `License server returned HTTP ${response.status}${text ? ": " + text.slice(0, 200) : ""}` };
    }

    const data = await response.json();
    return data;
  } catch (err) {
    return { ok: false, error: `Couldn't reach the license server: ${err.message}` };
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
      return { ok: false, error: `Local AI returned HTTP ${response.status}` };
    }

    const data = await response.json();
    const content = data.choices?.[0]?.message?.content;
    if (!content) {
      return { ok: false, error: "The local model didn't say anything back" };
    }

    return { ok: true, result: content };
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "The local AI took too long (60s timeout)" };
    }
    return { ok: false, error: `Couldn't reach your local AI: ${err.message}` };
  }
});

ipcMain.handle("agent:run-byok", async (_event, { prompt, taskComplexity, workflowType, byokKey }) => {
  if (!byokKey) {
    return { ok: false, error: "I need your OpenRouter key for this mode" };
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
      return { ok: false, error: "OpenRouter didn't return anything useful" };
    }

    return { ok: true, result: content };
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "That took too long (120s timeout)" };
    }
    return { ok: false, error: `OpenRouter request failed: ${err.message}` };
  }
});

ipcMain.handle("agent:run-standard", async (_event, { prompt, licenseKey, taskComplexity, workflowType, targetIntegrations }) => {
  if (!prompt || !licenseKey) {
    return { ok: false, error: "I need both a prompt and your license key" };
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
      return { ok: false, error: `Cloud engine returned HTTP ${response.status}${text ? ": " + text.slice(0, 200) : ""}` };
    }

    const data = await response.json();
    return data;
  } catch (err) {
    if (err.name === "AbortError") {
      return { ok: false, error: "The cloud took too long (120s timeout)" };
    }
    return { ok: false, error: `Couldn't reach the cloud: ${err.message}` };
  }
});

ipcMain.handle("download-gguf-model", async (_event, { repo, file }) => {
  const url = `https://huggingface.co/${repo}/resolve/main/${file}`;
  const destPath = path.join(modelsDir, file);

  try {
    if (fs.existsSync(destPath)) {
      return { ok: true, message: "You've already got this model!" };
    }

    const response = await fetch(url);
    if (!response.ok) {
      return { ok: false, error: `Hugging Face said no (HTTP ${response.status})` };
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
    return { ok: false, error: `Download didn't finish: ${err.message}` };
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
    return { ok: false, error: "Can't find the llama engine binary. Drop it in resources/bin/ and try again." };
  }

  if (!modelFile) {
    return { ok: false, error: "Pick a model first — head to Local AI Hub and choose one." };
  }

  const modelPath = path.join(modelsDir, modelFile);
  if (!fs.existsSync(modelPath)) {
    return { ok: false, error: "I can't find that model file. Download it from Local AI Hub first." };
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
      return { ok: false, error: "The local engine didn't start in time. Check your model path and logs." };
    }

    return { ok: true };
  } catch (err) {
    killLlamaProcess();
    return { ok: false, error: `Couldn't start the local engine: ${err.message}` };
  }
});

ipcMain.handle("integrations:dispatch", async (_event, { result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations }) => {
  try {
    const response = await fetch(`${backendUrl}/api/v1/integrations/dispatch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations, machineId: hardwareFingerprint }),
    });

    if (!response.ok) {
      const text = await response.text().catch(() => "");
      return { ok: false, error: `Integration server returned HTTP ${response.status}${text ? ": " + text.slice(0, 200) : ""}` };
    }

    const data = await response.json();
    return data;
  } catch (err) {
    return { ok: false, error: `Couldn't push to your apps: ${err.message}` };
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

  return { ok: false, error: "Didn't hear back from the app — try again?" };
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
