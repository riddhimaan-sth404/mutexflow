const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("api", {
  runAgent: (prompt, licenseKey, taskComplexity, workflowType, targetIntegrations) =>
    ipcRenderer.invoke("agent:run", { prompt, licenseKey, taskComplexity, workflowType, targetIntegrations }),
});
