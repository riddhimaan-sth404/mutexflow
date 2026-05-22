const fs = require("fs");
const path = require("path");

const backendUrl = process.env.BACKEND_URL || "http://localhost:3001";
const configPath = path.join(__dirname, "..", "config.js");

const content = `const BACKEND_URL = ${JSON.stringify(backendUrl)};\n\nmodule.exports = { BACKEND_URL };\n`;

fs.writeFileSync(configPath, content, "utf-8");
console.log(`[inject] BACKEND_URL set to ${backendUrl}`);
