const licenseKeyInput = document.getElementById("licenseKey");
const workflowTypeSelect = document.getElementById("workflowType");
const complexitySelect = document.getElementById("taskComplexity");
const promptInput = document.getElementById("promptInput");
const executeBtn = document.getElementById("executeBtn");
const logOutput = document.getElementById("logOutput");
const clearBtn = document.getElementById("clearBtn");
const spinner = document.getElementById("spinner");

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

async function runAgent() {
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

  appendLog("[SYSTEM] Sending request to agent...", "system");
  setLoading(true);

  try {
    appendLog("[SYSTEM] Workflow: " + workflowTypeSelect.options[workflowTypeSelect.selectedIndex].text, "system");
    const result = await window.api.runAgent(prompt, licenseKey, taskComplexity, workflowType);

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
  }
}
