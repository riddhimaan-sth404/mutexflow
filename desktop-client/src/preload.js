const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("api", {
  runAgent: (prompt, licenseKey, taskComplexity) =>
    ipcRenderer.invoke("agent:run", { prompt, licenseKey, taskComplexity }),
});
