import { serverRequest } from '@/utils/request';

export interface LlmProviderOption {
  id: string;
  label: string;
  baseUrl: string;
  active: boolean;
  modelOptions?: string[];
}

export type LlmParameterSupportState =
  | 'supported'
  | 'unsupported'
  | 'conditional'
  | 'fixed'
  | 'unknown';

export interface LlmProviderBaseUrl {
  label: string;
  url: string;
  region?: string;
  default?: boolean;
}

export interface LlmReasoningCapability {
  mode: string;
  requestField?: string;
  options?: string[];
  defaultValue?: string | number | boolean;
  disableValue?: string | number | boolean;
  dynamicValue?: string | number | boolean;
  min?: number;
  max?: number;
  notes?: string[];
}

export interface LlmProviderParameterSupport {
  temperature: LlmParameterSupportState;
  topP: LlmParameterSupportState;
  topK: LlmParameterSupportState;
  streaming: LlmParameterSupportState;
  imageInput: LlmParameterSupportState;
  textFileInput?: LlmParameterSupportState;
  binaryFileInput?: LlmParameterSupportState;
  pdfInput?: LlmParameterSupportState;
  videoInput?: LlmParameterSupportState;
  reasoning?: LlmReasoningCapability | null;
  notes?: string[];
}

export interface LlmProviderCatalogItem {
  id: string;
  label: string;
  apiStyle: string;
  docsUrl: string;
  authScheme: 'bearer' | 'x-api-key' | 'x-goog-api-key';
  defaultEndpoint?: string;
  baseUrls: LlmProviderBaseUrl[];
  modelDiscovery: 'static' | 'remote' | 'hybrid';
  modelListEndpoint?: string;
  sampleModels?: string[];
  parameterSupport: LlmProviderParameterSupport;
  notes?: string[];
}

export type LlmAttachmentKind = 'image' | 'text' | 'file';

export interface LlmChatAttachment {
  kind: LlmAttachmentKind;
  name: string;
  mediaType: string;
  size?: number;
  dataUrl?: string;
  text?: string;
}

export interface LlmConversationMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
  attachments?: LlmChatAttachment[];
}

export interface LlmChatSettings {
  enabled: boolean;
  provider: string;
  model: string;
  baseUrl: string;
  activeProviderId?: string;
  providerOptions: LlmProviderOption[];
  modelOptions?: string[];
  reasoningOptions?: string[];
  providerCatalog?: LlmProviderCatalogItem[];
  promptProfile: string;
  supports: {
    streaming: boolean;
    temperature: boolean;
    topP: boolean;
    topK: boolean;
    reasoningEffort: boolean;
    imageInput?: boolean;
    textFileInput?: boolean;
    binaryFileInput?: boolean;
  };
}

export interface LlmChatRequest {
  message: string;
  messages?: LlmConversationMessage[];
  attachments?: LlmChatAttachment[];
  baseUrl?: string;
  model?: string;
  temperature?: number;
  topP?: number;
  topK?: number;
  reasoningEffort?: string;
}

export interface LlmChatResponse {
  message: string;
  model: string;
  baseUrl: string;
  promptProfile: string;
}

export interface LlmManagedModel {
  id: string;
  enabled: boolean;
}

export interface LlmManagedProvider {
  id: string;
  label: string;
  providerId: string;
  providerLabel?: string;
  baseUrl: string;
  apiKey: string;
  timeoutSeconds: number;
  headers: Record<string, string>;
  models: LlmManagedModel[];
  active: boolean;
}

export interface LlmManagerState {
  activeProviderId?: string;
  configPath: string;
  providerCatalog: LlmProviderCatalogItem[];
  providers: LlmManagedProvider[];
}

export interface LlmManagerSaveRequest {
  activeProviderId?: string;
  providers: LlmManagedProvider[];
}

export interface LlmManagerFetchModelsResponse {
  source: string;
  provider: LlmManagedProvider;
  models: LlmManagedModel[];
}

export interface LlmModelTestResult {
  modelId: string;
  ok: boolean;
  latencyMs: number;
  error?: string;
}

export interface LlmManagerTestModelsResponse {
  provider: LlmManagedProvider;
  results: LlmModelTestResult[];
}

export interface LlmManagerPreviewResponse {
  provider: LlmManagedProvider;
  modelId: string;
  method: string;
  endpoint: string;
  headers: Record<string, string>;
  body: unknown;
  bodyText: string;
}

export default class LlmManager {
  public static async getSettings () {
    const { data } = await serverRequest.get<ServerResponse<LlmChatSettings>>('/LLM/GetSettings');
    return data.data;
  }

  public static async chat (payload: LlmChatRequest) {
    const { data } = await serverRequest.post<ServerResponse<LlmChatResponse>>('/LLM/Chat', payload, {
      timeout: 120000,
    });
    return data.data;
  }

  public static async getManagerState () {
    const { data } = await serverRequest.get<ServerResponse<LlmManagerState>>('/LLM/GetManagerState');
    return data.data;
  }

  public static async saveManagerState (payload: LlmManagerSaveRequest) {
    const { data } = await serverRequest.post<ServerResponse<LlmManagerState>>('/LLM/SaveManagerState', payload);
    return data.data;
  }

  public static async fetchModels (provider: LlmManagedProvider) {
    const { data } = await serverRequest.post<ServerResponse<LlmManagerFetchModelsResponse>>('/LLM/FetchModels', {
      provider,
    });
    return data.data;
  }

  public static async testModels (
    provider: LlmManagedProvider,
    options?: {
      modelId?: string;
      testAll?: boolean;
    }
  ) {
    const { data } = await serverRequest.post<ServerResponse<LlmManagerTestModelsResponse>>('/LLM/TestModels', {
      provider,
      modelId: options?.modelId,
      testAll: options?.testAll ?? false,
    }, {
      timeout: 120000,
    });
    return data.data;
  }

  public static async previewRequest (
    provider: LlmManagedProvider,
    modelId?: string
  ) {
    const { data } = await serverRequest.post<ServerResponse<LlmManagerPreviewResponse>>('/LLM/PreviewRequest', {
      provider,
      modelId,
    });
    return data.data;
  }
}
