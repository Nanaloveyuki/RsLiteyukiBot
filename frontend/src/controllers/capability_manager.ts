import { serverRequest } from '@/utils/request';

export interface ToolInventoryItem {
  name: string;
  description?: string;
  parameters?: unknown;
  category?: string;
  origin?: string;
  whenToUse?: string;
  strict?: boolean;
  active?: boolean;
}

export interface ToolInventoryResponse {
  tools: ToolInventoryItem[];
  warnings: string[];
}

export interface ToolToggleResponse extends ToolInventoryResponse {
  name: string;
  active: boolean;
  configPath?: string;
}

export interface McpServerItem {
  name: string;
  transport: string;
  url: string;
  active: boolean;
  toolCount?: number;
  toolNames?: string[];
  warnings?: string[];
  headers?: Record<string, string>;
}

export interface McpServersResponse {
  configPath?: string;
  servers: McpServerItem[];
  warnings: string[];
}

export interface SkillInventoryItem {
  name: string;
  description?: string;
  path?: string;
}

export interface SkillsResponse {
  skills: SkillInventoryItem[];
  warnings: string[];
  managedRoot?: string;
}

export interface SkillReadResponse extends SkillInventoryItem {
  content: string;
  truncated: boolean;
}

export interface SkillUploadRequest {
  name: string;
  content: string;
  overwrite: boolean;
}

export interface SkillImportResponse {
  skills: SkillInventoryItem[];
  count: number;
  managedRoot?: string;
}

export default class CapabilityManager {
  public static async getTools () {
    const { data } = await serverRequest.get<ServerResponse<ToolInventoryResponse>>('/tools');
    return data.data;
  }

  public static async toggleTool (name: string, active: boolean) {
    const { data } = await serverRequest.post<ServerResponse<ToolToggleResponse>>('/tools/toggle', {
      name,
      active,
    });
    return data.data;
  }

  public static async getMcpServers () {
    const { data } = await serverRequest.get<ServerResponse<McpServersResponse>>('/mcp/servers');
    return data.data;
  }

  public static async saveMcpServers (servers: McpServerItem[]) {
    const { data } = await serverRequest.post<ServerResponse<McpServersResponse>>('/mcp/save', {
      servers,
    });
    return data.data;
  }

  public static async testMcpServer (server: McpServerItem) {
    const { data } = await serverRequest.post<ServerResponse<McpServersResponse>>('/mcp/test', {
      server,
    });
    return data.data;
  }

  public static async getSkills () {
    const { data } = await serverRequest.get<ServerResponse<SkillsResponse>>('/skills');
    return data.data;
  }

  public static async readSkill (name: string) {
    const { data } = await serverRequest.get<ServerResponse<SkillReadResponse>>('/skills/read', {
      params: { name },
    });
    return data.data;
  }

  public static async uploadSkill (payload: SkillUploadRequest) {
    const { data } = await serverRequest.post<ServerResponse<SkillInventoryItem>>('/skills/upload', payload);
    return data.data;
  }

  public static async importSkills (files: File[], overwrite: boolean) {
    const formData = new FormData();
    files.forEach((file) => {
      const relativePath = (file as File & { webkitRelativePath?: string; }).webkitRelativePath?.trim();
      formData.append('skill', file, relativePath || file.name);
    });
    formData.append('overwrite', overwrite ? 'true' : 'false');

    const { data } = await serverRequest.post<ServerResponse<SkillImportResponse>>('/skills/import', formData, {
      timeout: 120000,
    });
    return data.data;
  }
}
