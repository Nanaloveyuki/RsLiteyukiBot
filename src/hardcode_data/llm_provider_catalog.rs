use serde_json::{Value, json};

pub(crate) fn provider_catalog() -> Vec<Value> {
    vec![
        json!({
            "id": "openai",
            "label": "OpenAI",
            "apiStyle": "openai-responses",
            "docsUrl": "https://platform.openai.com/docs/api-reference/responses",
            "authScheme": "bearer",
            "defaultEndpoint": "/responses",
            "baseUrls": [{ "label": "OpenAI Public", "url": "https://api.openai.com/v1", "default": true }],
            "modelDiscovery": "hybrid",
            "sampleModels": ["gpt-5", "gpt-5-mini", "gpt-5-nano", "gpt-5.1", "gpt-4.1"],
            "parameterSupport": {
                "temperature": "supported",
                "topP": "supported",
                "topK": "unsupported",
                "frequencyPenalty": "supported",
                "presencePenalty": "supported",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "supported",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "effort",
                    "requestField": "reasoning.effort",
                    "options": ["none", "minimal", "low", "medium", "high", "xhigh"],
                    "disableValue": "none",
                    "notes": [
                        "不同 OpenAI 模型支持的 effort 集合不同，应按模型再裁剪",
                        "topK 未出现在 OpenAI 官方参数文档中，这里按 direct OpenAI 不支持处理"
                    ]
                }
            }
        }),
        json!({
            "id": "anthropic",
            "label": "Anthropic",
            "apiStyle": "anthropic-messages",
            "docsUrl": "https://docs.anthropic.com/en/api/messages",
            "authScheme": "x-api-key",
            "defaultEndpoint": "/v1/messages",
            "baseUrls": [{ "label": "Anthropic Public", "url": "https://api.anthropic.com", "default": true }],
            "modelDiscovery": "static",
            "sampleModels": ["claude-opus-4-1-20250805", "claude-opus-4-20250514", "claude-sonnet-4-20250514", "claude-3-7-sonnet-20250219"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "conditional",
                "frequencyPenalty": "unsupported",
                "presencePenalty": "unsupported",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "unsupported",
                "binaryFileInput": "unsupported",
                "reasoning": {
                    "mode": "budget_tokens",
                    "requestField": "thinking.budget_tokens",
                    "min": 1024,
                    "notes": [
                        "思考模式通过 thinking 对象开启，而不是 effort 字符串",
                        "启用 thinking 时 temperature 不能设置，top_k 也不兼容，top_p 需固定为 1 或不超过 0.95"
                    ]
                }
            }
        }),
        json!({
            "id": "google-gemini",
            "label": "Google Gemini",
            "apiStyle": "google-gemini",
            "docsUrl": "https://ai.google.dev/gemini-api/docs/text-generation",
            "authScheme": "x-goog-api-key",
            "defaultEndpoint": "/v1beta/models/{model}:generateContent",
            "baseUrls": [{ "label": "Generative Language API", "url": "https://generativelanguage.googleapis.com/v1beta", "default": true }],
            "modelDiscovery": "static",
            "sampleModels": ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-3-flash-preview"],
            "parameterSupport": {
                "temperature": "supported",
                "topP": "supported",
                "topK": "supported",
                "frequencyPenalty": "unsupported",
                "presencePenalty": "unsupported",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "supported",
                "binaryFileInput": "conditional",
                "pdfInput": "supported",
                "reasoning": {
                    "mode": "provider_specific",
                    "notes": [
                        "Gemini 2.5 系列用 thinkingBudget",
                        "Gemini 3 系列用 thinkingConfig.thinkingLevel",
                        "是否可完全关闭思考取决于具体模型，例如 2.5 Flash 可设为 0，2.5 Pro 不能关闭"
                    ]
                }
            }
        }),
        json!({
            "id": "openrouter",
            "label": "OpenRouter",
            "apiStyle": "openrouter-chat",
            "docsUrl": "https://openrouter.ai/docs/api-reference/overview",
            "authScheme": "bearer",
            "defaultEndpoint": "/chat/completions",
            "baseUrls": [{ "label": "OpenRouter", "url": "https://openrouter.ai/api/v1", "default": true }],
            "modelDiscovery": "hybrid",
            "modelListEndpoint": "https://openrouter.ai/api/v1/models",
            "sampleModels": ["openai/gpt-5", "anthropic/claude-sonnet-4", "google/gemini-2.5-pro"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "conditional",
                "frequencyPenalty": "conditional",
                "presencePenalty": "conditional",
                "streaming": "supported",
                "imageInput": "conditional",
                "textFileInput": "conditional",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "model_metadata",
                    "notes": [
                        "OpenRouter 不适合下发固定 reasoningOptions，应以 /models 返回的 supported_parameters 为准",
                        "不同上游模型参数支持范围不同"
                    ]
                }
            }
        }),
        json!({
            "id": "kimi",
            "label": "Kimi API",
            "apiStyle": "openai-chat",
            "docsUrl": "https://platform.moonshot.ai/docs/guide/use-kimi-k2-thinking-model.en-US",
            "authScheme": "bearer",
            "defaultEndpoint": "/chat/completions",
            "baseUrls": [{ "label": "Moonshot Public", "url": "https://api.moonshot.ai/v1", "default": true }],
            "modelDiscovery": "hybrid",
            "sampleModels": ["kimi-k2.5", "kimi-k2-thinking", "kimi-latest-8k", "kimi-latest-128k"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "conditional",
                "frequencyPenalty": "conditional",
                "presencePenalty": "conditional",
                "streaming": "supported",
                "imageInput": "supported",
                "textFileInput": "supported",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "enabled_disabled_or_dedicated_model",
                    "notes": [
                        "kimi-k2.5 默认开启 thinking capability，也可切换 disabled",
                        "kimi-k2-thinking 是强制思考模型",
                        "K2.5 模型的 temperature 与 top_p 为固定值，需后端按模型禁用对应滑杆"
                    ]
                }
            }
        }),
        json!({
            "id": "qwen",
            "label": "Qwen API",
            "apiStyle": "openai-chat",
            "docsUrl": "https://www.alibabacloud.com/help/en/model-studio/use-qwen-by-calling-api",
            "authScheme": "bearer",
            "defaultEndpoint": "/compatible-mode/v1/chat/completions",
            "baseUrls": [
                { "label": "Beijing", "url": "https://dashscope.aliyuncs.com/compatible-mode/v1", "default": true },
                { "label": "Singapore", "url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", "region": "ap-southeast-1" },
                { "label": "US Virginia", "url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", "region": "us-east-1" }
            ],
            "modelDiscovery": "hybrid",
            "sampleModels": ["qwen-max", "qwen-plus", "qwen-turbo", "qwen3-max"],
            "parameterSupport": {
                "temperature": "conditional",
                "topP": "conditional",
                "topK": "unknown",
                "frequencyPenalty": "conditional",
                "presencePenalty": "conditional",
                "streaming": "supported",
                "imageInput": "conditional",
                "textFileInput": "conditional",
                "binaryFileInput": "conditional",
                "reasoning": {
                    "mode": "enable_thinking_and_budget",
                    "requestField": "extra_body.enable_thinking",
                    "notes": [
                        "Qwen OpenAI 兼容模式通过 extra_body.enable_thinking 控制深度思考",
                        "Qwen3-Max 等模型还支持 thinking_budget"
                    ]
                }
            }
        }),
    ]
}
