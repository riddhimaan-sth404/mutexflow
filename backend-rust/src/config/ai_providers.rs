use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudProvider {
    pub name: &'static str,
    pub url: &'static str,
    pub model: &'static str,
    pub env_key: &'static str,
}

pub const CLOUD_CASCADES: &[CloudProvider] = &[
    CloudProvider {
        name: "Cerebras",
        url: "https://api.cerebras.ai/v1/chat/completions",
        model: "llama3.1-8b",
        env_key: "CEREBRAS_API_KEY",
    },
    CloudProvider {
        name: "Groq",
        url: "https://api.groq.com/openai/v1/chat/completions",
        model: "llama-3.3-70b-versatile",
        env_key: "GROQ_API_KEY",
    },
    CloudProvider {
        name: "Google Studio",
        url: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
        model: "gemini-1.5-flash",
        env_key: "GEMINI_API_KEY",
    },
    CloudProvider {
        name: "SambaNova",
        url: "https://api.sambanova.ai/v1/chat/completions",
        model: "Meta-Llama-3.1-405B-Instruct",
        env_key: "SAMBANOVA_API_KEY",
    },
    CloudProvider {
        name: "SiliconFlow",
        url: "https://api.siliconflow.cn/v1/chat/completions",
        model: "deepseek-ai/DeepSeek-V3",
        env_key: "SILICONFLOW_API_KEY",
    },
    CloudProvider {
        name: "DeepSeek Official",
        url: "https://api.deepseek.com/v1/chat/completions",
        model: "deepseek-chat",
        env_key: "DEEPSEEK_API_KEY",
    },
    CloudProvider {
        name: "Together AI",
        url: "https://api.together.xyz/v1/chat/completions",
        model: "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
        env_key: "TOGETHER_API_KEY",
    },
    CloudProvider {
        name: "Fireworks AI",
        url: "https://api.fireworks.ai/inference/v1/chat/completions",
        model: "accounts/fireworks/models/llama-v3p1-8b-instruct",
        env_key: "FIREWORKS_API_KEY",
    },
    CloudProvider {
        name: "Novita AI",
        url: "https://api.novita.ai/v3/openai/chat/completions",
        model: "meta-llama/llama-3.1-8b-instruct",
        env_key: "NOVITA_API_KEY",
    },
    CloudProvider {
        name: "Hugging Face Serverless",
        url: "https://api-inference.huggingface.co/v1/chat/completions",
        model: "meta-llama/Meta-Llama-3-8B-Instruct",
        env_key: "HF_API_KEY",
    },
];
