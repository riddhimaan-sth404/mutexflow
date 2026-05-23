const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("api", {
  verifyLicense: (licenseKey) =>
    ipcRenderer.invoke("auth:verify-license", { licenseKey }),
  runAgentLocal: (params) =>
    ipcRenderer.invoke("agent:run-local", params),
  runAgentByok: (params) =>
    ipcRenderer.invoke("agent:run-byok", params),
  runAgentStandard: (params) =>
    ipcRenderer.invoke("agent:run-standard", params),
  dispatchIntegrations: (params) =>
    ipcRenderer.invoke("integrations:dispatch", params),
  downloadGgufModel: (params) =>
    ipcRenderer.invoke("download-gguf-model", params),
  spawnLlamaCpp: (params) =>
    ipcRenderer.invoke("spawn-llama-cpp", params),
  onDownloadProgress: (callback) => {
    const handler = (_event, data) => callback(data);
    ipcRenderer.on("download-progress", handler);
    return () => ipcRenderer.removeListener("download-progress", handler);
  },
  openOAuthPopup: (provider) =>
    ipcRenderer.invoke("oauth:connect", { provider }),
});
