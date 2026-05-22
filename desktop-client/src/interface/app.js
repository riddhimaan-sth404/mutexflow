const licenseKeyInput = document.getElementById("licenseKey");
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

const COOLDOWN_MAP = {
  low: 1,
  medium: 3,
  high: 8,
  reasoning: 8
};

const DEFAULT_BTN_TEXT = "Run Agent";

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

  appendLog("[SYSTEM] Sending request to agent...", "system");
  setLoading(true);

  try {
    appendLog("[SYSTEM] Workflow: " + workflowTypeSelect.options[workflowTypeSelect.selectedIndex].text, "system");
    if (targetIntegrations.length > 0) {
      appendLog("[SYSTEM] Integrations: " + targetIntegrations.join(", "), "system");
    }
    const result = await window.api.runAgent(prompt, licenseKey, taskComplexity, workflowType, targetIntegrations);

    if (result.ok) {
      appendLog("[OK]", "ok");
      appendLog(result.result, "ok");
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
