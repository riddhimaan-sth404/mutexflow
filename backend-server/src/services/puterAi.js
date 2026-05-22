const { AGENT_PROMPTS } = require("./agentPrompts");

const MODEL_MAP = {
  low: ["gpt-5.4-nano", "gemini-2.5-flash-lite"],
  medium: ["google/gemini-1.5-flash"],
  high: ["meta/llama-3.3-70b", "deepseek/deepseek-r1"],
  reasoning: ["meta/llama-3.3-70b", "deepseek/deepseek-r1"],
};

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function buildMessages(prompt, workflowType) {
  const entry = AGENT_PROMPTS[workflowType] || AGENT_PROMPTS.general_reasoning;
  return [
    { role: "system", content: entry.system },
    { role: "user", content: prompt },
  ];
}

async function runAi(prompt, complexity = "medium", workflowType = "general_reasoning") {
  const messages = buildMessages(prompt, workflowType);
  const models = MODEL_MAP[complexity] || MODEL_MAP.medium;
  const maxRetries = 3;
  const baseDelay = 1000;

  for (const model of models) {
    for (let attempt = 0; attempt < maxRetries; attempt++) {
      try {
        const response = await puter.ai.chat(messages, { model });

        console.log(
          "[DEBUG]",
          JSON.stringify({
            workflowType,
            complexity,
            model,
            response,
            attempt: attempt + 1,
          })
        );

        return { success: true, data: response };
      } catch (err) {
        const isRateLimit =
          err?.status === 429 ||
          (err?.message && err.message.includes("429")) ||
          (err?.message && err.message.toLowerCase().includes("rate limit"));

        if (isRateLimit && attempt < maxRetries - 1) {
          const delay = baseDelay * Math.pow(2, attempt);
          console.warn(
            `[429] Rate limited on ${model}, retrying in ${delay}ms (attempt ${attempt + 1}/${maxRetries})`
          );
          await sleep(delay);
          continue;
        }

        console.error(
          `[ERROR] Model ${model} failed:`,
          err?.message || err
        );
        break;
      }
    }
  }

  return { success: false, error: "All models exhausted after retries" };
}

module.exports = { runAi };
