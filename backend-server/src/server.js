require("dotenv").config();

const token = process.env.PUTER_AUTH_TOKEN;
if (!token) {
  console.error("[FATAL] PUTER_AUTH_TOKEN is required");
  process.exit(1);
}

require("@heyputer/puter.js/src/init.cjs").init(token);

const express = require("express");
const cors = require("cors");
const agentRouter = require("./routes/agent");

const app = express();

app.use(cors());
app.use(express.json({ limit: "1mb" }));
app.use("/api/v1", agentRouter);

const PORT = process.env.PORT || 3001;
app.listen(PORT, () => {
  console.log(`[backend] B2B AI Proxy running on port ${PORT}`);
});
