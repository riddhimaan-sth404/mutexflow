const CLOUD_CASCADES = [
  { name: 'Cerebras', url: 'https://api.cerebras.ai/v1/chat/completions', model: 'llama3.1-8b', key: process.env.CEREBRAS_API_KEY },
  { name: 'Groq', url: 'https://api.groq.com/openai/v1/chat/completions', model: 'llama-3.3-70b-versatile', key: process.env.GROQ_API_KEY },
  { name: 'Google Studio', url: 'https://generativelanguage.googleapis.com/v1beta/openai/chat/completions', model: 'gemini-1.5-flash', key: process.env.GEMINI_API_KEY },
  { name: 'SambaNova', url: 'https://api.sambanova.ai/v1/chat/completions', model: 'Meta-Llama-3.1-405B-Instruct', key: process.env.SAMBANOVA_API_KEY },
  { name: 'SiliconFlow', url: 'https://api.siliconflow.cn/v1/chat/completions', model: 'deepseek-ai/DeepSeek-V3', key: process.env.SILICONFLOW_API_KEY },
  { name: 'DeepSeek Official', url: 'https://api.deepseek.com/v1/chat/completions', model: 'deepseek-chat', key: process.env.DEEPSEEK_API_KEY },
  { name: 'Together AI', url: 'https://api.together.xyz/v1/chat/completions', model: 'meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo', key: process.env.TOGETHER_API_KEY },
  { name: 'Fireworks AI', url: 'https://api.fireworks.ai/inference/v1/chat/completions', model: 'accounts/fireworks/models/llama-v3p1-8b-instruct', key: process.env.FIREWORKS_API_KEY },
  { name: 'Novita AI', url: 'https://api.novita.ai/v3/openai/chat/completions', model: 'meta-llama/llama-3.1-8b-instruct', key: process.env.NOVITA_API_KEY },
  { name: 'Hugging Face Serverless', url: 'https://api-inference.huggingface.co/v1/chat/completions', model: 'meta-llama/Meta-Llama-3-8B-Instruct', key: process.env.HF_API_KEY }
];

module.exports = { CLOUD_CASCADES };
