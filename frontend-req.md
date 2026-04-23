# Frontend Req For `模型对话` Page

这个文档只描述当前前端 `frontend/src/pages/dashboard/debug/websocket/index.tsx` 所依赖的后端接口契约，供后端实现对接。

## 统一返回包裹

前端沿用现有 WebUI 返回格式：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

- `code === 0` 视为成功。
- `code !== 0` 时前端直接抛错并展示 `message`。

## 1. 获取模型设置

- Method: `GET /api/LLM/GetSettings`
- 用途:
  - 初始化当前 Provider / Model
  - 获取可切换的 Provider / Model / 推理强度选项
  - 告知前端是否支持 `temperature` / `top_p` / `top_k` / 图片 / 文件
  - 下发一份 provider catalog，描述不同平台的官方接口风格、鉴权方式、参数差异和推荐模型

### 返回示例

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "enabled": true,
    "provider": "OpenAI Compatible",
    "baseUrl": "https://api.openai.com/v1",
    "model": "gpt-5-mini",
    "providerOptions": [
      {
        "id": "openai",
        "label": "OpenAI",
        "baseUrl": "https://api.openai.com/v1",
        "active": true
      },
      {
        "id": "claude-proxy",
        "label": "Claude Proxy",
        "baseUrl": "https://example.com/claude/v1",
        "active": false
      }
    ],
    "modelOptions": [
      "gpt-5-mini",
      "gpt-5",
      "claude-sonnet-4-20250514"
    ],
    "reasoningOptions": [
      "low",
      "medium",
      "high",
      "xhigh",
      "max"
    ],
    "promptProfile": "default",
    "supports": {
      "streaming": false,
      "temperature": true,
      "topP": true,
      "topK": true,
      "reasoningEffort": true,
      "imageInput": true,
      "textFileInput": true,
      "binaryFileInput": true
    },
    "providerCatalog": [
      {
        "id": "openai",
        "label": "OpenAI",
        "apiStyle": "openai-responses",
        "docsUrl": "https://platform.openai.com/docs/api-reference/responses",
        "authScheme": "bearer",
        "defaultEndpoint": "/responses",
        "baseUrls": [
          {
            "label": "OpenAI Public",
            "url": "https://api.openai.com/v1",
            "default": true
          }
        ],
        "modelDiscovery": "static",
        "sampleModels": [
          "gpt-5",
          "gpt-5-mini",
          "gpt-5-nano",
          "gpt-5.1",
          "gpt-4.1"
        ],
        "parameterSupport": {
          "temperature": "supported",
          "topP": "supported",
          "topK": "unsupported",
          "streaming": "supported",
          "imageInput": "supported",
          "textFileInput": "supported",
          "binaryFileInput": "conditional",
          "reasoning": {
            "mode": "effort",
            "requestField": "reasoning.effort",
            "options": [
              "minimal",
              "low",
              "medium",
              "high",
              "xhigh"
            ],
            "notes": [
              "不同 OpenAI 模型支持的 effort 集合不同，应按模型再裁剪",
              "topK 未出现在 OpenAI 官方参数文档中，这里按 direct OpenAI 不支持处理"
            ]
          }
        }
      },
      {
        "id": "anthropic",
        "label": "Anthropic",
        "apiStyle": "anthropic-messages",
        "docsUrl": "https://docs.anthropic.com/en/api/messages",
        "authScheme": "x-api-key",
        "defaultEndpoint": "/v1/messages",
        "baseUrls": [
          {
            "label": "Anthropic Public",
            "url": "https://api.anthropic.com",
            "default": true
          }
        ],
        "modelDiscovery": "static",
        "sampleModels": [
          "claude-opus-4-1-20250805",
          "claude-opus-4-20250514",
          "claude-sonnet-4-20250514",
          "claude-3-7-sonnet-20250219"
        ],
        "parameterSupport": {
          "temperature": "conditional",
          "topP": "conditional",
          "topK": "conditional",
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
      },
      {
        "id": "google-gemini",
        "label": "Google Gemini",
        "apiStyle": "google-gemini",
        "docsUrl": "https://ai.google.dev/gemini-api/docs/text-generation",
        "authScheme": "x-goog-api-key",
        "defaultEndpoint": "/v1beta/models/{model}:generateContent",
        "baseUrls": [
          {
            "label": "Generative Language API",
            "url": "https://generativelanguage.googleapis.com/v1beta",
            "default": true
          }
        ],
        "modelDiscovery": "static",
        "sampleModels": [
          "gemini-2.5-pro",
          "gemini-2.5-flash",
          "gemini-2.5-flash-lite",
          "gemini-3-flash-preview"
        ],
        "parameterSupport": {
          "temperature": "supported",
          "topP": "supported",
          "topK": "supported",
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
      },
      {
        "id": "openrouter",
        "label": "OpenRouter",
        "apiStyle": "openrouter-chat",
        "docsUrl": "https://openrouter.ai/docs/api-reference/overview",
        "authScheme": "bearer",
        "defaultEndpoint": "/chat/completions",
        "baseUrls": [
          {
            "label": "OpenRouter",
            "url": "https://openrouter.ai/api/v1",
            "default": true
          }
        ],
        "modelDiscovery": "remote",
        "modelListEndpoint": "https://openrouter.ai/api/v1/models",
        "sampleModels": [
          "openai/gpt-5",
          "anthropic/claude-sonnet-4",
          "google/gemini-2.5-pro"
        ],
        "parameterSupport": {
          "temperature": "conditional",
          "topP": "conditional",
          "topK": "conditional",
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
      },
      {
        "id": "kimi",
        "label": "Kimi API",
        "apiStyle": "openai-chat",
        "docsUrl": "https://platform.moonshot.ai/docs/guide/use-kimi-k2-thinking-model.en-US",
        "authScheme": "bearer",
        "defaultEndpoint": "/chat/completions",
        "baseUrls": [
          {
            "label": "Moonshot Public",
            "url": "https://api.moonshot.ai/v1",
            "default": true
          }
        ],
        "modelDiscovery": "static",
        "sampleModels": [
          "kimi-k2.5",
          "kimi-k2-thinking",
          "kimi-latest-8k",
          "kimi-latest-128k"
        ],
        "parameterSupport": {
          "temperature": "conditional",
          "topP": "conditional",
          "topK": "conditional",
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
      },
      {
        "id": "qwen",
        "label": "Qwen API",
        "apiStyle": "openai-chat",
        "docsUrl": "https://www.alibabacloud.com/help/en/model-studio/use-qwen-by-calling-api",
        "authScheme": "bearer",
        "defaultEndpoint": "/compatible-mode/v1/chat/completions",
        "baseUrls": [
          {
            "label": "Beijing",
            "url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "default": true
          },
          {
            "label": "Singapore",
            "url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            "region": "ap-southeast-1"
          },
          {
            "label": "US Virginia",
            "url": "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            "region": "us-east-1"
          }
        ],
        "modelDiscovery": "static",
        "sampleModels": [
          "qwen-max",
          "qwen-plus",
          "qwen-turbo",
          "qwen3-max"
        ],
        "parameterSupport": {
          "temperature": "conditional",
          "topP": "conditional",
          "topK": "unknown",
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
      }
    ]
  }
}
```

### 字段约定

- `enabled: boolean`
  - `false` 时前端禁用发送按钮。
- `provider: string`
  - 当前 Provider 的显示名。
- `baseUrl: string`
  - 当前实际使用的 Provider 地址。
- `model: string`
  - 当前默认模型。
- `providerOptions: Array<{ id, label, baseUrl, active }>`
  - Provider 切换列表。
- `modelOptions?: string[]`
  - 可选的模型列表。
  - 若为空，前端仍允许手动输入模型字符串。
- `reasoningOptions?: string[]`
  - 推理强度候选，必须由后端下发字符串列表。
  - 不要让前端写死枚举，因为不同平台可能返回 `xhigh`、`max` 等不同值。
- `providerCatalog?: ProviderCatalogItem[]`
  - 用来描述后端支持的各个 LLM 平台模板。
  - 这不是当前会话“已启用的实例列表”，而是一个官方能力元数据目录。
  - 建议后端至少内置 `openai`、`anthropic`、`google-gemini`、`openrouter`、`kimi`、`qwen`。
- `promptProfile: string`
  - 当前使用的 prompt profile，仅展示。
- `supports`
  - 控制对应设置项是否可编辑，以及附件支持文案。

### `ProviderCatalogItem`

```json
{
  "id": "openai",
  "label": "OpenAI",
  "apiStyle": "openai-responses",
  "docsUrl": "https://platform.openai.com/docs/api-reference/responses",
  "authScheme": "bearer",
  "defaultEndpoint": "/responses",
  "baseUrls": [
    {
      "label": "OpenAI Public",
      "url": "https://api.openai.com/v1",
      "default": true
    }
  ],
  "modelDiscovery": "static | remote | hybrid",
  "modelListEndpoint": "optional",
  "sampleModels": [
    "gpt-5",
    "gpt-5-mini"
  ],
  "parameterSupport": {
    "temperature": "supported | unsupported | conditional | fixed | unknown",
    "topP": "supported | unsupported | conditional | fixed | unknown",
    "topK": "supported | unsupported | conditional | fixed | unknown",
    "streaming": "supported | unsupported | conditional | fixed | unknown",
    "imageInput": "supported | unsupported | conditional | fixed | unknown",
    "textFileInput": "supported | unsupported | conditional | fixed | unknown",
    "binaryFileInput": "supported | unsupported | conditional | fixed | unknown",
    "reasoning": {
      "mode": "string",
      "requestField": "optional string",
      "options": [
        "optional"
      ],
      "min": 0,
      "max": 0,
      "defaultValue": "optional",
      "disableValue": "optional",
      "dynamicValue": "optional",
      "notes": [
        "optional"
      ]
    }
  },
  "notes": [
    "optional"
  ]
}
```

### 后端下发策略建议

- `providerOptions`
  - 代表“当前用户已配置/可切换的实例”。
- `providerCatalog`
  - 代表“系统知道的官方 Provider 模板与能力差异”。
- `modelOptions`
  - 对于 `openrouter` 建议实时拉 `/models` 后缓存。
  - 对于 `openai`、`anthropic`、`google-gemini`、`kimi`、`qwen` 可以先走后端维护的静态白名单。
- `reasoningOptions`
  - 应该是“当前选中 provider + 当前选中 model”的最终可选项，不应是全局固定数组。
  - 例如:
    - OpenAI GPT-5 可返回 `["minimal","low","medium","high","xhigh"]`
    - Claude 不应返回 effort 字符串，而应让 `supports.reasoningEffort` 为 `false`
    - Gemini 也不建议复用 effort 字符串，而应由后端自行映射成适合当前 UI 的选择器

## 2. 发起对话

- Method: `POST /api/LLM/Chat`
- 用途:
  - 单次发送当前输入
  - 同时把前端当前会话历史一并传给后端，实现多轮对话

### 请求体

```json
{
  "message": "帮我分析这张图片",
  "messages": [
    {
      "role": "user",
      "content": "上一轮问题",
      "attachments": []
    },
    {
      "role": "assistant",
      "content": "上一轮回答"
    },
    {
      "role": "user",
      "content": "帮我分析这张图片",
      "attachments": [
        {
          "kind": "image",
          "name": "clipboard.png",
          "mediaType": "image/png",
          "size": 182331,
          "dataUrl": "data:image/png;base64,..."
        }
      ]
    }
  ],
  "attachments": [
    {
      "kind": "image",
      "name": "clipboard.png",
      "mediaType": "image/png",
      "size": 182331,
      "dataUrl": "data:image/png;base64,..."
    }
  ],
  "baseUrl": "https://api.openai.com/v1",
  "model": "gpt-5-mini",
  "reasoningEffort": "xhigh",
  "temperature": 0.7,
  "topP": 0.95,
  "topK": 32
}
```

### 请求字段说明

- `message: string`
  - 当前这一轮用户输入的纯文本内容。
  - 允许为空字符串，前端支持“只发附件”。
- `messages?: Array<Message>`
  - 当前完整会话历史，包含本轮新消息。
  - 后端应优先使用它来实现真正的多轮上下文。
- `attachments?: Attachment[]`
  - 当前这一轮用户消息的附件列表。
- `baseUrl?: string`
  - 当前用户选择的 Provider 地址。
- `model?: string`
  - 当前用户选择的模型。
- `reasoningEffort?: string`
  - 直接透传前端当前所选字符串，不做前端枚举限制。
- `temperature?: number`
- `topP?: number`
- `topK?: number`

### `Message`

```json
{
  "role": "user | assistant | system",
  "content": "string",
  "attachments": []
}
```

### `Attachment`

```json
{
  "kind": "image | text | file",
  "name": "string",
  "mediaType": "string",
  "size": 123,
  "dataUrl": "optional string",
  "text": "optional string"
}
```

### 附件约定

- `kind === "image"`
  - 前端传 `dataUrl`
- `kind === "text"`
  - 前端传 `text`
- `kind === "file"`
  - 前端传 `dataUrl`
  - 主要用于普通文件占位，后端可自行决定是否消费或拒绝

## 3. 对话返回

### 成功返回示例

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "message": "这是模型回复",
    "model": "gpt-5-mini",
    "baseUrl": "https://api.openai.com/v1",
    "promptProfile": "default"
  }
}
```

### 字段说明

- `message: string`
  - 助手最终回复文本。
- `model: string`
  - 实际使用的模型名，前端展示在消息元信息里。
- `baseUrl: string`
  - 实际使用的 Provider 地址。
- `promptProfile: string`
  - 实际使用的 prompt profile。

## 4. 当前前端交互点

后端实现时请按下面前端行为考虑：

- 页面是聊天优先布局，参数都在设置弹窗里。
- 输入框原生支持文本粘贴。
- `Ctrl+V` 如果带图片或文件，前端会拦截并加入附件区。
- `+` 按钮可添加图片和文件。
- 前端把会话历史 `messages` 和本轮附件 `attachments` 一并传输。
- 当前页面按一次请求拿一次完整回复，前端暂未做流式渲染。

## 5. 官方文档依据

以下信息按 2026-04-23 检索的官方文档整理，后端实现时可直接参考这些入口：

- OpenAI
  - Responses API: https://platform.openai.com/docs/api-reference/responses
  - 模型与 reasoning: https://platform.openai.com/docs/models
  - 图片输入: https://platform.openai.com/docs/guides/images
- Anthropic
  - Messages API: https://docs.anthropic.com/en/api/messages
  - 模型列表: https://docs.anthropic.com/en/docs/about-claude/models
  - Extended thinking: https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking
- Google Gemini
  - Text generation / GenerateContent: https://ai.google.dev/gemini-api/docs/text-generation
  - Thinking: https://ai.google.dev/gemini-api/docs/thinking
  - 模型列表: https://ai.google.dev/gemini-api/docs/models
- OpenRouter
  - API overview: https://openrouter.ai/docs/api-reference/overview
  - Models API: https://openrouter.ai/docs/api-reference/list-available-models
- Kimi API
  - API 概览: https://platform.moonshot.ai/docs/api-reference
  - Thinking models: https://platform.moonshot.ai/docs/guide/use-kimi-k2-thinking-model.en-US
  - Disable thinking capability: https://platform.moonshot.ai/docs/guide/disable-thinking-capability-of-kimi-k2-5.en-US
  - Image understanding: https://platform.moonshot.ai/docs/guide/image-understanding
- Qwen API
  - 调用 Qwen API: https://www.alibabacloud.com/help/en/model-studio/use-qwen-by-calling-api
  - DashScope OpenAI 兼容: https://www.alibabacloud.com/help/en/model-studio/compatibility-of-openai-with-dashscope
  - Deep thinking: https://www.alibabacloud.com/help/en/model-studio/deep-thinking
