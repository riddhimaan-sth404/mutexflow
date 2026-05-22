const {
  app,
  BrowserWindow,
  ipcMain,
  safeStorage,
} = require("electron");
const path = require("path");
const { BACKEND_URL } = require("./config");

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
      preload: path.join(__dirname, "preload.js"),
    },
  });

  mainWindow.loadFile(path.join(__dirname, "interface", "index.html"));
}

ipcMain.handle("agent:run", async (_event, { prompt, licenseKey, taskComplexity }) => {
  if (!prompt || !licenseKey) {
    return { ok: false, error: "Prompt and license key are required" };
  }

  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 120000);

    const response = await fetch(`${backendUrl}/api/v1/agent/run`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ prompt, licenseKey, taskComplexity }),
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

app.whenReady().then(createWindow);

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});
