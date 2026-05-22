const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("api", {
  runAgent: (prompt, licenseKey, taskComplexity, workflowType) =>
    ipcRenderer.invoke("agent:run", { prompt, licenseKey, taskComplexity, workflowType }),
});
