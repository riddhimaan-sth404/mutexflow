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
const integSlack = document.getElementById("integSlack");
const integSheets = document.getElementById("integSheets");
const integGithub = document.getElementById("integGithub");
const accountsToggle = document.getElementById("accountsToggle");
const accountsPanel = document.getElementById("accountsPanel");
const authBtns = document.querySelectorAll(".auth-btn");

const MODE_STORAGE_KEY = "mutexflow_inference_mode";
const BYOK_KEY_STORAGE_KEY = "mutexflow_byok_key";

const COOLDOWN_MAP = {
  low: 1,
  medium: 3,
  high: 8,
  reasoning: 8
};

const DEFAULT_BTN_TEXT = "Run Agent";

const connectedAccounts = {};

const STATUS_IDS = {
  slack: "statusSlack",
  "google-sheets": "statusSheets",
  github: "statusGithub",
};

const savedMode = localStorage.getItem(MODE_STORAGE_KEY);
if (savedMode) {
  inferenceMode.value = savedMode;
}

const savedByokKey = localStorage.getItem(BYOK_KEY_STORAGE_KEY);
if (savedByokKey) {
  byokKeyInput.value = savedByokKey;
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

accountsToggle.addEventListener("click", () => {
  accountsPanel.classList.toggle("hidden");
  accountsToggle.classList.toggle("active");
});

authBtns.forEach((btn) => {
  btn.addEventListener("click", async () => {
    const provider = btn.dataset.provider;
    const licenseKey = licenseKeyInput.value.trim();
    if (!licenseKey) {
      appendLog("[SYSTEM] Enter a license key before connecting accounts.", "system");
      return;
    }
    const connectionId = "org-" + licenseKey.slice(0, 8);
    btn.disabled = true;
    btn.textContent = "Connecting...";
    try {
      const result = await window.api.nangoAuth(provider, connectionId);
      if (result.ok) {
        connectedAccounts[provider] = result.connectionId || connectionId;
        const statusEl = document.getElementById(STATUS_IDS[provider]);
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
      btn.textContent = "Connect " + (provider === "slack" ? "Slack Workspace" : provider === "google-sheets" ? "Google Account" : "GitHub Account");
    }
  });
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

  const targetIntegrations = [];
  if (integSlack.checked) targetIntegrations.push(integSlack.value);
  if (integSheets.checked) targetIntegrations.push(integSheets.value);
  if (integGithub.checked) targetIntegrations.push(integGithub.value);

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

      if (!result.ok) {
        appendLog("[SYSTEM] Local Core unavailable. Start llama.cpp with a GGUF model on port 8080, or switch to Cloud mode.", "system");
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
