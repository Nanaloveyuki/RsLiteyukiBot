import { serverRequest } from '@/utils/request';
import { PluginStoreList, PluginStoreItem } from '@/types/plugin-store';

/** 插件状态 */
export type PluginStatus = 'active' | 'disabled' | 'stopped';
export type PluginRuntimeKind = 'native' | 'python' | 'lua' | 'external';
export type PluginSourceKind =
  | 'liteyuki-native'
  | 'liteyuki-python-bridge'
  | 'astrbot-compatible'
  | 'runtime-lua'
  | 'runtime-external';

export interface PluginCapabilityFlags {
  any: boolean;
  tools: boolean;
  webApis: boolean;
  cronJobs: boolean;
  tasks: boolean;
}

/** 插件信息 */
export interface PluginItem {
  /** 显示名称 (优先 package.json 的 plugin 字段) */
  name: string;
  /** 包名 (package name)，用于 API 操作 */
  id: string;
  /** 版本号 */
  version: string;
  /** 描述 */
  description: string;
  /** 作者 */
  author: string;
  /** 状态: active-运行中, disabled-已禁用, stopped-已停止 */
  status: PluginStatus;
  /** 是否有配置项 */
  hasConfig?: boolean;
  /** 是否有扩展页面 */
  hasPages?: boolean;
  runtimeKind?: PluginRuntimeKind;
  pluginType?: string;
  sourceKind?: PluginSourceKind;
  compatKind?: string;
  hasCapabilities?: PluginCapabilityFlags;
  /** 主页链接 */
  homepage?: string;
  /** 仓库链接 */
  repository?: string;
  /** 插件图标 URL（由后端返回） */
  icon?: string;
}

/** 扩展页面信息 */
export interface ExtensionPageItem {
  /** 插件 ID */
  pluginId: string;
  /** 插件名称 */
  pluginName: string;
  /** 页面路径 */
  path: string;
  /** 页面标题 */
  title: string;
  /** 页面图标 */
  icon?: string;
  /** 页面描述 */
  description?: string;
}

/** 插件列表响应 */
export interface PluginListResponse {
  plugins: PluginItem[];
  pluginManagerNotFound: boolean;
  extensionPages: ExtensionPageItem[];
}

/** 插件配置项定义 */
export interface PluginConfigSchemaItem {
  key: string;
  type: 'string' | 'number' | 'boolean' | 'select' | 'multi-select' | 'html' | 'text';
  label: string;
  description?: string;
  default?: any;
  options?: { label: string; value: string | number; }[];
  placeholder?: string;
  /** 标记此字段为响应式：值变化时触发 schema 刷新 */
  reactive?: boolean;
  /** 是否隐藏此字段 */
  hidden?: boolean;
}

/** 插件配置响应 */
export interface PluginConfigResponse {
  schema: PluginConfigSchemaItem[];
  config: Record<string, unknown>;
  /** 是否支持响应式更新 */
  supportReactive?: boolean;
}

export interface PluginCapabilitySupportState {
  registered: boolean;
  executable: boolean;
  persistent: boolean;
  active: boolean;
  status: 'registered_only' | 'deferred' | 'active' | 'disabled' | 'error' | 'unsupported' | string;
}

export interface PluginCapabilitySupportSummary {
  tools: PluginCapabilitySupportState;
  webApis: PluginCapabilitySupportState;
  cronJobs: PluginCapabilitySupportState;
  tasks: PluginCapabilitySupportState;
}

export interface PluginCapabilitySnapshot {
  pluginId: string;
  runtimeKind: PluginRuntimeKind;
  tools: unknown[];
  webApis: unknown[];
  cronJobs: unknown[];
  tasks: unknown[];
  updatedAt?: string;
}

export interface PluginCapabilitiesResponse {
  pluginId: string;
  runtimeKind: PluginRuntimeKind;
  support: PluginCapabilitySupportSummary;
  snapshot: PluginCapabilitySnapshot;
}

export interface PluginCapabilityListResponse {
  pluginId: string;
  runtimeKind: PluginRuntimeKind;
  support: PluginCapabilitySupportState;
  items: unknown[];
  updatedAt?: string;
}

export interface PluginRuntimeBindingSummary {
  tools: boolean;
  webApis: boolean;
  cronJobs: boolean;
  tasks: boolean;
}

export interface PluginRuntimeStateResponse {
  pluginId: string;
  runtimeKind: PluginRuntimeKind;
  loaded: boolean;
  enabled: boolean;
  active: boolean;
  snapshotExtracted: boolean;
  executableBindings: PluginRuntimeBindingSummary;
  schedulerStatus: string;
  taskRuntimeStatus: string;
}

export interface PluginExecutionRecord {
  lastError?: string;
  lastErrorAt?: string;
  lastSuccessAt?: string;
}

export interface PluginDiagnosticsResponse {
  pluginId: string;
  runtimeKind: PluginRuntimeKind;
  loadState: string;
  snapshotExtracted: boolean;
  executableBindings: PluginRuntimeBindingSummary;
  schedulerStatus: string;
  lastWebApiDispatch: PluginExecutionRecord;
  lastToolExecution: PluginExecutionRecord;
  lastCronExecution: PluginExecutionRecord;
}

/** 服务端响应 */
export interface ServerResponse<T> {
  code: number;
  message: string;
  data: T;
}

/**
 * 插件管理器 API
 */
export default class PluginManager {
  /**
   * 获取插件列表
   */
  public static async getPluginList (): Promise<PluginListResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginListResponse>>('/Plugin/List');
    return data.data;
  }

  /**
   * 手动注册插件管理器到 NetworkManager
   */
  public static async registerPluginManager (): Promise<{ message: string; }> {
    const { data } = await serverRequest.post<ServerResponse<{ message: string; }>>('/Plugin/RegisterManager');
    return data.data;
  }

  /**
   * 设置插件状态（启用/禁用）
   * @param id 插件包名
   * @param enable 是否启用
   */
  public static async setPluginStatus (id: string, enable: boolean): Promise<void> {
    await serverRequest.post<ServerResponse<void>>('/Plugin/SetStatus', { id, enable });
  }

  /**
   * 卸载插件
   * @param id 插件包名
   * @param cleanData 是否清理数据
   */
  public static async uninstallPlugin (id: string, cleanData?: boolean): Promise<void> {
    await serverRequest.post<ServerResponse<void>>('/Plugin/Uninstall', { id, cleanData });
  }

  /**
   * 导入本地插件包
   * @param file 插件 zip 文件
   */
  public static async importLocalPlugin (file: File): Promise<{ message: string; pluginId: string; installPath: string; }> {
    const formData = new FormData();
    formData.append('plugin', file);

    const { data } = await serverRequest.post<ServerResponse<{ message: string; pluginId: string; installPath: string; }>>(
      '/Plugin/Import',
      formData,
      {
        timeout: 60000, // 60秒超时
      }
    );
    return data.data;
  }

  // ==================== 插件商店 ====================

  /**
   * 获取插件商店列表
   * @param forceRefresh 是否强制刷新（跳过服务端缓存）
   */
  public static async getPluginStoreList (forceRefresh: boolean = false): Promise<PluginStoreList> {
    const params = forceRefresh ? { forceRefresh: 'true' } : {};
    const { data } = await serverRequest.get<ServerResponse<PluginStoreList>>('/Plugin/Store/List', { params });
    return data.data;
  }

  /**
   * 获取插件商店详情
   * @param id 插件 ID
   */
  public static async getPluginStoreDetail (id: string): Promise<PluginStoreItem> {
    const { data } = await serverRequest.get<ServerResponse<PluginStoreItem>>(`/Plugin/Store/Detail/${id}`);
    return data.data;
  }

  /**
   * 从商店安装插件
   * @param id 插件 ID
   * @param mirror 镜像源
   */
  public static async installPluginFromStore (id: string, mirror?: string): Promise<void> {
    await serverRequest.post<ServerResponse<void>>(
      '/Plugin/Store/Install',
      { id, mirror },
      { timeout: 300000 } // 5分钟超时
    );
  }

  // ==================== 插件配置 ====================

  /**
   * 获取插件配置
   * @param id 插件包名
   */
  public static async getPluginConfig (id: string): Promise<PluginConfigResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginConfigResponse>>('/Plugin/Config', {
      params: { id },
    });
    return data.data;
  }

  /**
   * 设置插件配置
   * @param id 插件包名
   * @param config 配置内容
   */
  public static async setPluginConfig (id: string, config: Record<string, unknown>): Promise<void> {
    await serverRequest.post<ServerResponse<void>>('/Plugin/Config', { id, config });
  }

  public static async getPluginCapabilities (id: string): Promise<PluginCapabilitiesResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginCapabilitiesResponse>>('/Plugin/Capabilities', {
      params: { id },
    });
    return data.data;
  }

  public static async getPluginTools (id: string): Promise<PluginCapabilityListResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginCapabilityListResponse>>('/Plugin/Tools', {
      params: { id },
    });
    return data.data;
  }

  public static async getPluginWebApis (id: string): Promise<PluginCapabilityListResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginCapabilityListResponse>>('/Plugin/WebApis', {
      params: { id },
    });
    return data.data;
  }

  public static async getPluginCronJobs (id: string): Promise<PluginCapabilityListResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginCapabilityListResponse>>('/Plugin/CronJobs', {
      params: { id },
    });
    return data.data;
  }

  public static async getPluginTasks (id: string): Promise<PluginCapabilityListResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginCapabilityListResponse>>('/Plugin/Tasks', {
      params: { id },
    });
    return data.data;
  }

  public static async getPluginRuntimeState (id: string): Promise<PluginRuntimeStateResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginRuntimeStateResponse>>('/Plugin/RuntimeState', {
      params: { id },
    });
    return data.data;
  }

  public static async getPluginDiagnostics (id: string): Promise<PluginDiagnosticsResponse> {
    const { data } = await serverRequest.get<ServerResponse<PluginDiagnosticsResponse>>('/Plugin/Diagnostics', {
      params: { id },
    });
    return data.data;
  }

  /**
   * 通知配置字段变化
   * @param id 插件包名
   * @param sessionId SSE 会话 ID
   * @param key 变化的字段
   * @param value 新值
   * @param currentConfig 当前配置
   */
  public static async notifyConfigChange (
    id: string,
    sessionId: string,
    key: string,
    value: unknown,
    currentConfig: Record<string, unknown>
  ): Promise<void> {
    await serverRequest.post<ServerResponse<void>>('/Plugin/Config/Change', {
      id,
      sessionId,
      key,
      value,
      currentConfig,
    });
  }

  /**
   * 获取配置 SSE URL
   * @param id 插件包名
   * @param config 初始配置
   */
  public static getConfigSSEUrl (id: string, config?: Record<string, unknown>): string {
    const params = new URLSearchParams({ id });
    if (config) {
      params.set('config', JSON.stringify(config));
    }
    return `/api/Plugin/Config/SSE?${params.toString()}`;
  }
}
