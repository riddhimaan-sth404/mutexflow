const licenseKeyInput = document.getElementById("licenseKey");
const inferenceMode = document.getElementById("inferenceMode");
const byokKeyInput = document.getElementById("byokKey");
const byokKeyRow = document.getElementById("byokKeyRow");
const workflowTypeSelect = document.getElementById("workflowType");
const complexitySelect = document.getElementById("taskComplexity");
const promptInput = document.getElementById("promptInput");
const executeBtn = document.getElementById("executeBtn");
const logOutput = document.getElementById("logOutput");
const clearBtn = document.getElementById("clearBtn");
const spinner = document.getElementById("spinner");
const localAiToggle = document.getElementById("localAiToggle");
const localAiPanel = document.getElementById("localAiPanel");
const localModelSelect = document.getElementById("localModelSelect");
const downloadModelBtn = document.getElementById("downloadModelBtn");
const downloadProgressBar = document.getElementById("downloadProgressBar");
const downloadStatusText = document.getElementById("downloadStatusText");
const integrationsHub = document.getElementById("integrationsHub");
const hubSearch = document.getElementById("hubSearch");

const MODE_STORAGE_KEY = "mutexflow_inference_mode";
const BYOK_KEY_STORAGE_KEY = "mutexflow_byok_key";
const LOCAL_MODEL_STORAGE_KEY = "mutexflow_local_model";

const COOLDOWN_MAP = {
  low: 1,
  medium: 3,
  high: 8,
  reasoning: 8
};

const DEFAULT_BTN_TEXT = "Run Agent";

let removeProgressListener = null;

const connectedAccounts = {};

const savedMode = localStorage.getItem(MODE_STORAGE_KEY);
if (savedMode) {
  inferenceMode.value = savedMode;
}

const savedByokKey = localStorage.getItem(BYOK_KEY_STORAGE_KEY);
if (savedByokKey) {
  byokKeyInput.value = savedByokKey;
}

const savedLocalModel = localStorage.getItem(LOCAL_MODEL_STORAGE_KEY);
if (savedLocalModel) {
  for (const opt of localModelSelect.options) {
    if (opt.value === savedLocalModel) {
      opt.selected = true;
      break;
    }
  }
}

function updateModeVisibility() {
  const mode = inferenceMode.value;
  localStorage.setItem(MODE_STORAGE_KEY, mode);
  if (mode === "byok") {
    byokKeyRow.classList.remove("hidden");
    localStorage.setItem(BYOK_KEY_STORAGE_KEY, byokKeyInput.value);
  } else {
    byokKeyRow.classList.add("hidden");
  }
}

updateModeVisibility();

inferenceMode.addEventListener("change", updateModeVisibility);

byokKeyInput.addEventListener("input", () => {
  if (inferenceMode.value === "byok") {
    localStorage.setItem(BYOK_KEY_STORAGE_KEY, byokKeyInput.value);
  }
});

localAiToggle.addEventListener("click", () => {
  localAiPanel.classList.toggle("hidden");
  localAiToggle.classList.toggle("active");
});

localModelSelect.addEventListener("change", () => {
  if (localModelSelect.value) {
    localStorage.setItem(LOCAL_MODEL_STORAGE_KEY, localModelSelect.value);
  }
});

downloadModelBtn.addEventListener("click", async () => {
  const selected = localModelSelect.options[localModelSelect.selectedIndex];
  if (!selected || !selected.value) {
    appendLog("[SYSTEM] Select a GGUF model from the list first.", "system");
    return;
  }

  const repo = selected.dataset.repo;
  const file = selected.dataset.file;

  downloadModelBtn.disabled = true;
  downloadModelBtn.textContent = "Downloading...";
  downloadProgressBar.style.width = "0%";
  downloadStatusText.textContent = "Starting download...";

  if (removeProgressListener) {
    removeProgressListener();
  }

  removeProgressListener = window.api.onDownloadProgress((data) => {
    const pct = data.total > 0 ? ((data.bytes / data.total) * 100).toFixed(1) : 0;
    downloadProgressBar.style.width = Math.min(pct, 100) + "%";
    const mb = (data.bytes / 1024 / 1024).toFixed(1);
    const totalMb = (data.total / 1024 / 1024).toFixed(1);
    downloadStatusText.textContent = mb + " MB / " + totalMb + " MB (" + pct + "%)";
  });

  try {
    const result = await window.api.downloadGgufModel({ repo, file });
    if (result.ok) {
      downloadProgressBar.style.width = "100%";
      downloadStatusText.textContent = "Download complete: " + file;
      appendLog("[SYSTEM] Model downloaded: " + file, "system");
    } else {
      downloadStatusText.textContent = "Download failed: " + (result.error || "Unknown error");
      appendLog("[ERROR] Model download failed: " + (result.error || "Unknown error"), "error");
    }
  } catch (err) {
    downloadStatusText.textContent = "Download error: " + err.message;
    appendLog("[ERROR] Model download error: " + err.message, "error");
  } finally {
    downloadModelBtn.disabled = false;
    downloadModelBtn.textContent = "Download and Setup";
  }
});

clearBtn.addEventListener("click", () => {
  logOutput.textContent = "";
});

executeBtn.addEventListener("click", runAgent);

promptInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    runAgent();
  }
});

hubSearch.addEventListener("input", () => {
  const query = hubSearch.value.toLowerCase().trim();
  const cards = integrationsHub.querySelectorAll(".integ-card");
  for (const card of cards) {
    const name = card.querySelector(".integ-card-name");
    if (name) {
      const match = name.textContent.toLowerCase().includes(query);
      card.style.display = match ? "" : "none";
    }
  }
});

integrationsHub.addEventListener("click", async (e) => {
  const btn = e.target.closest(".auth-btn");
  if (!btn) return;

  const provider = btn.dataset.provider;
  btn.disabled = true;
  btn.textContent = "Connecting...";
  try {
    const result = await window.api.openOAuthPopup(provider);
    if (result.ok) {
      connectedAccounts[provider] = provider;
      const statusEl = document.getElementById("status-" + provider);
      if (statusEl) {
        statusEl.textContent = "Connected";
        statusEl.className = "auth-status connected";
      }
      appendLog("[SYSTEM] " + provider + " workspace connected.", "system");
    } else {
      appendLog("[SYSTEM] " + provider + " auth failed: " + (result.error || "cancelled"), "system");
    }
  } catch (err) {
    appendLog("[SYSTEM] " + provider + " auth error: " + err.message, "system");
  } finally {
    btn.disabled = false;
    btn.textContent = "Connect";
  }
});

function appendLog(text, type) {
  const line = document.createElement("span");
  line.className = "log-line" + (type ? " " + type : "");
  line.textContent = text;
  logOutput.appendChild(line);
  logOutput.scrollTop = logOutput.scrollHeight;
}

function setLoading(loading) {
  executeBtn.disabled = loading;
  spinner.classList.toggle("hidden", !loading);
}

function startCooldown(seconds) {
  executeBtn.disabled = true;
  executeBtn.classList.add("btn-cooldown");
  let remaining = seconds;

  function tick() {
    if (remaining <= 0) {
      executeBtn.textContent = DEFAULT_BTN_TEXT;
      executeBtn.disabled = false;
      executeBtn.classList.remove("btn-cooldown");
      return;
    }
    executeBtn.textContent = "Cooling down (" + remaining + "s)...";
    remaining--;
    setTimeout(tick, 1000);
  }

  tick();
}

function getTargetIntegrations() {
  const result = [];
  const checked = integrationsHub.querySelectorAll(".integ-card-checkbox:checked");
  for (const cb of checked) {
    result.push(cb.value);
  }
  return result;
}

async function runAgent() {
  if (executeBtn.disabled) return;

  const prompt = promptInput.value.trim();
  const licenseKey = licenseKeyInput.value.trim();
  const mode = inferenceMode.value;
  const taskComplexity = complexitySelect.value;
  const workflowType = workflowTypeSelect.value;

  if (!prompt) {
    appendLog("[SYSTEM] Please enter a prompt or command.", "system");
    return;
  }

  if (!licenseKey) {
    appendLog("[SYSTEM] Please enter your license key.", "system");
    return;
  }

  const targetIntegrations = getTargetIntegrations();

  appendLog("[SYSTEM] Verifying license...", "system");
  setLoading(true);

  try {
    const licenseResult = await window.api.verifyLicense(licenseKey);
    if (!licenseResult.ok) {
      appendLog("[ERROR] " + (licenseResult.error || "License verification failed"), "error");
      setLoading(false);
      startCooldown(COOLDOWN_MAP[taskComplexity] || 3);
      return;
    }
    appendLog("[SYSTEM] License verified.", "system");

    appendLog("[SYSTEM] Mode: " + inferenceMode.options[inferenceMode.selectedIndex].text, "system");
    appendLog("[SYSTEM] Workflow: " + workflowTypeSelect.options[workflowTypeSelect.selectedIndex].text, "system");
    if (targetIntegrations.length > 0) {
      appendLog("[SYSTEM] Integrations: " + targetIntegrations.join(", "), "system");
    }

    let result;

    if (mode === "local") {
      appendLog("[SYSTEM] Routing to Local Core (llama.cpp)...", "system");
      result = await window.api.runAgentLocal({ prompt, taskComplexity, workflowType });

      if (!result.ok && result.error && result.error.toLowerCase().includes("connection refused")) {
        appendLog("[SYSTEM] Local llama.cpp service is offline. Booting the binary engine automatically...", "system");
        const selected = localModelSelect.options[localModelSelect.selectedIndex];
        const modelFile = selected && selected.value ? selected.dataset.file : null;
        const bootResult = await window.api.spawnLlamaCpp({ modelFile });

        if (bootResult.ok) {
          appendLog("[SYSTEM] llama.cpp engine started. Retrying inference...", "system");
          await new Promise(r => setTimeout(r, 2000));
          result = await window.api.runAgentLocal({ prompt, taskComplexity, workflowType });
        } else {
          appendLog("[ERROR] Failed to boot llama.cpp: " + (bootResult.error || "Unknown error"), "error");
          setLoading(false);
          startCooldown(COOLDOWN_MAP[taskComplexity] || 3);
          return;
        }
      }

      if (!result.ok) {
        appendLog("[SYSTEM] Local Core unavailable. Download a GGUF model and ensure llama.cpp can start on port 8080, or switch to Cloud mode.", "system");
        setLoading(false);
        startCooldown(COOLDOWN_MAP[taskComplexity] || 3);
        return;
      }
    } else if (mode === "byok") {
      const byokKey = localStorage.getItem(BYOK_KEY_STORAGE_KEY) || byokKeyInput.value.trim();
      if (!byokKey) {
        appendLog("[ERROR] Enter your OpenRouter key in the User OpenRouter Key field.", "error");
        setLoading(false);
        startCooldown(COOLDOWN_MAP[taskComplexity] || 3);
        return;
      }
      appendLog("[SYSTEM] Routing to Cloud BYOK (OpenRouter)...", "system");
      result = await window.api.runAgentByok({ prompt, taskComplexity, workflowType, byokKey });
    } else {
      appendLog("[SYSTEM] Routing to Cloud Standard cascade...", "system");
      result = await window.api.runAgentStandard({ prompt, licenseKey, taskComplexity, workflowType, targetIntegrations });
    }

    if (result.ok) {
      appendLog("[OK]", "ok");
      appendLog(result.result, "ok");

      if (targetIntegrations.length > 0 && (mode === "local" || mode === "byok")) {
        appendLog("[SYSTEM] Dispatching integrations...", "system");
        await window.api.dispatchIntegrations({ result: result.result, workflowType, taskComplexity, prompt, licenseKey, targetIntegrations });
      }
    } else {
      appendLog("[ERROR] " + (result.error || "Unknown error"), "error");
    }
  } catch (err) {
    appendLog("[FATAL] " + err.message, "error");
  } finally {
    setLoading(false);
    startCooldown(COOLDOWN_MAP[taskComplexity] || 3);
  }
}
