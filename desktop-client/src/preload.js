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
  nangoAuth: (provider, connectionId) =>
    ipcRenderer.invoke("nango:auth", { provider, connectionId }),
});
