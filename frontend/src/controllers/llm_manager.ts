import { serverRequest } from '@/utils/request';

export interface LlmProviderOption {
  id: string;
  label: string;
  baseUrl: string;
  active: boolean;
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
}
