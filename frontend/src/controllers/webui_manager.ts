import CryptoJS from 'crypto-js';
import { EventSourcePolyfill } from 'event-source-polyfill';

import { LogLevel } from '@/const/enum';
import { buildBearerAuthHeader, readStoredAuthToken } from '@/utils/auth';
import { parseLogLevel } from '@/utils/terminal';
import { resolveApiUrl } from '@/utils/runtime';

import { serverRequest } from '@/utils/request';

export interface Log {
  level: LogLevel;
  message: string;
}

export interface ReleaseAssetInfo {
  name: string;
  contentType: string;
  size: number;
  downloadCount: number;
  downloadUrl: string;
  mirrorDownloadUrl?: string;
  createdAt: string;
  updatedAt: string;
}

export interface ReleaseVersionInfo {
  tag: string;
  name?: string;
  type: 'release' | 'prerelease';
  htmlUrl: string;
  mirrorHtmlUrl?: string;
  body?: string;
  createdAt: string;
  publishedAt?: string;
  assets: ReleaseAssetInfo[];
  recommendedAsset?: ReleaseAssetInfo;
}

export interface ReleasePlatformInfo {
  os: string;
  arch: string;
  displayName: string;
}

export interface ReleaseListResponse {
  platform: ReleasePlatformInfo;
  currentVersion: string;
  versions: ReleaseVersionInfo[];
  pagination: {
    page: number;
    pageSize: number;
    total: number;
    totalPages: number;
  };
  mirror?: string;
  repoUrl: string;
  releasesUrl: string;
}

export interface LatestReleaseResponse {
  platform: ReleasePlatformInfo;
  currentVersion: string;
  latest: ReleaseVersionInfo;
  mirror?: string;
  repoUrl: string;
  releasesUrl: string;
}

export interface EventStreamHandle {
  close: () => void;
}

function parseRealtimeLogBatch(message: string, fallbackLevel: LogLevel) {
  return message
    .replace(/\r\n/g, '\n')
    .split('\n')
    .map((line) => line.trimEnd())
    .filter((line) => line.trim().length > 0)
    .map((line) => ({
      level: parseLogLevel(line, fallbackLevel),
      message: line,
    }));
}

function createManagedEventSource (
  path: string,
  onMessage: (event: MessageEvent<string>) => void
): EventStreamHandle {
  let eventSource: EventSourcePolyfill | null = null;
  let reconnectTimer: number | null = null;
  let closed = false;
  let retryDelayMs = 1000;

  const clearReconnectTimer = () => {
    if (reconnectTimer !== null) {
      window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  const scheduleReconnect = () => {
    if (closed || reconnectTimer !== null) {
      return;
    }

    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, retryDelayMs);
    retryDelayMs = Math.min(retryDelayMs * 2, 10000);
  };

  const connect = () => {
    if (closed) {
      return;
    }

    const token = readStoredAuthToken();
    if (!token) {
      scheduleReconnect();
      return;
    }

    eventSource = new EventSourcePolyfill(resolveApiUrl(path), {
      headers: {
        ...buildBearerAuthHeader(token),
        Accept: 'text/event-stream',
      },
    });

    eventSource.onopen = () => {
      retryDelayMs = 1000;
    };

    eventSource.onmessage = (event) => {
      onMessage(event as MessageEvent<string>);
    };

    eventSource.onerror = (error) => {
      console.error('SSE连接出错:', error);
      eventSource?.close();
      eventSource = null;
      scheduleReconnect();
    };
  };

  connect();

  return {
    close: () => {
      closed = true;
      clearReconnectTimer();
      eventSource?.close();
      eventSource = null;
    },
  };
}

export default class WebUIManager {
  public static async checkWebUiLogined () {
    const { data } =
      await serverRequest.post<ServerResponse<boolean>>('/auth/check');
    return data.data;
  }

  public static async getAuthState () {
    const { data } =
      await serverRequest.get<ServerResponse<WebUiAuthState>>('/auth/state');
    return data.data;
  }

  public static async loginWithToken (token: string) {
    const sha256 = CryptoJS.SHA256(token + '.napcat').toString();
    const { data } = await serverRequest.post<ServerResponse<AuthResponse>>(
      '/auth/login',
      { hash: sha256 }
    );
    return data.data.Credential;
  }

  public static async loginWithPassword (password: string) {
    const { data } = await serverRequest.post<ServerResponse<AuthResponse>>(
      '/auth/login/password',
      { password }
    );
    return data.data.Credential;
  }

  public static async changePassword (oldPassword: string, newPassword: string) {
    const { data } = await serverRequest.post<ServerResponse<boolean>>(
      '/auth/update_password',
      { oldPassword, newPassword }
    );
    return data.data;
  }

  public static async proxy<T> (url = '') {
    const data = await serverRequest.get<ServerResponse<string>>(
      '/base/proxy?url=' + encodeURIComponent(url)
    );
    data.data.data = JSON.parse(data.data.data);
    return data.data as ServerResponse<T>;
  }

  public static async getAppVersion () {
    const { data } =
      await serverRequest.get<ServerResponse<PackageInfo>>('/base/GetAppVersion');
    return data.data;
  }

  public static async getLatestRelease (mirror?: string) {
    const { data } =
      await serverRequest.get<ServerResponse<LatestReleaseResponse>>('/base/GetLatestRelease', {
        params: { mirror },
      });
    return data.data;
  }

  /**
   * 版本信息接口
   */
  static readonly VersionTypes = {
    RELEASE: 'release',
    PRERELEASE: 'prerelease',
    ACTION: 'action',
  } as const;

  /**
   * 获取所有可用的版本列表（支持分页、过滤和搜索）
   * 懒加载：根据 type 参数只获取对应类型的版本
   */
  public static async getAllReleases (options: {
    page?: number;
    pageSize?: number;
    type?: 'release' | 'prerelease' | 'all';
    search?: string;
    mirror?: string;
  } = {}) {
    const { page = 1, pageSize = 20, type = 'release', search = '', mirror } = options;
    const { data } = await serverRequest.get<ServerResponse<ReleaseListResponse>>('/base/getAllReleases', {
      params: { page, pageSize, type, search, mirror },
    });
    return data.data;
  }

  public static async getMirrors () {
    const { data } =
      await serverRequest.get<ServerResponse<{ mirrors: string[]; }>>('/base/getMirrors');
    return data.data;
  }

  public static async getQQVersion () {
    const { data } =
      await serverRequest.get<ServerResponse<string>>('/base/QQVersion');
    return data.data;
  }

  public static async getThemeConfig () {
    const { data } =
      await serverRequest.get<ServerResponse<ThemeConfig>>('/base/Theme');
    return data.data;
  }

  public static async setThemeConfig (theme: ThemeConfig) {
    const { data } = await serverRequest.post<ServerResponse<boolean>>(
      '/base/SetTheme',
      { theme }
    );
    return data.data;
  }

  public static async restart () {
    const { data } = await serverRequest.post<ServerResponse<any>>('/Process/Restart');
    return data.data;
  }

  public static async getAllUsers (): Promise<any> {
    const { data } = await serverRequest.get<ServerResponse<any>>('/QQLogin/GetAllUsers');
    return data.data;
  }

  public static async getLogList () {
    const { data } =
      await serverRequest.get<ServerResponse<string[]>>('/Log/GetLogList');
    return data.data;
  }

  public static async getLogContent (logName: string) {
    const { data } = await serverRequest.get<ServerResponse<string>>(
      `/Log/GetLog?id=${logName}`
    );
    return data.data;
  }

  public static getRealTimeLogs (writer: (data: Log[]) => void): EventStreamHandle {
    if (!readStoredAuthToken()) {
      throw new Error('未登录');
    }
    return createManagedEventSource('/Log/GetLogRealTime', (event) => {
      try {
        const data = JSON.parse(event.data) as Log;
        const logs = parseRealtimeLogBatch(data.message, data.level);
        if (logs.length > 0) {
          writer(logs);
        }
      } catch (error) {
        console.error(error);
      }
    });
  }

  public static getSystemStatus (writer: (data: SystemStatus) => void): EventStreamHandle {
    if (!readStoredAuthToken()) {
      throw new Error('未登录');
    }
    return createManagedEventSource('/base/GetSysStatusRealTime', (event) => {
      try {
        const data = JSON.parse(event.data) as SystemStatus;
        writer(data);
      } catch (error) {
        console.error(error);
      }
    });
  }

  // 获取WebUI基础配置
  public static async getWebUIConfig () {
    const { data } = await serverRequest.get<ServerResponse<WebUIConfig>>(
      '/WebUIConfig/GetConfig'
    );
    return data.data;
  }

  // 更新WebUI基础配置
  public static async updateWebUIConfig (config: Partial<WebUIConfig>) {
    const { data } = await serverRequest.post<ServerResponse<boolean>>(
      '/WebUIConfig/UpdateConfig',
      config
    );
    return data.data;
  }

  public static async getWebUIAppearance () {
    const { data } = await serverRequest.get<ServerResponse<WebUIAppearanceState>>(
      '/WebUIConfig/GetAppearance'
    );
    return data.data;
  }

  public static async updateWebUIAppearance (appearance: WebUIAppearanceState) {
    const { data } = await serverRequest.post<ServerResponse<WebUIAppearanceState>>(
      '/WebUIConfig/UpdateAppearance',
      appearance
    );
    return data.data;
  }

  // 获取是否禁用WebUI
  public static async getDisableWebUI () {
    const { data } = await serverRequest.get<ServerResponse<boolean>>(
      '/WebUIConfig/GetDisableWebUI'
    );
    return data.data;
  }

  // 更新是否禁用WebUI
  public static async updateDisableWebUI (disable: boolean) {
    const { data } = await serverRequest.post<ServerResponse<boolean>>(
      '/WebUIConfig/UpdateDisableWebUI',
      { disable }
    );
    return data.data;
  }

  public static async getDesktopSettings () {
    const { data } = await serverRequest.get<ServerResponse<DesktopSettings>>(
      '/Desktop/GetSettings'
    );
    return data.data;
  }

  public static async updateDesktopSettings (settings: Pick<DesktopSettings, 'closeToTray'>) {
    const { data } = await serverRequest.post<ServerResponse<DesktopSettings>>(
      '/Desktop/UpdateSettings',
      settings
    );
    return data.data;
  }

  public static async getActiveAppConfig () {
    const { data } = await serverRequest.get<ServerResponse<ActiveAppConfigState>>(
      '/AppConfig/GetActive'
    );
    return data.data;
  }

  public static async replaceActiveAppConfig (content: string) {
    const { data } = await serverRequest.post<ServerResponse<ActiveAppConfigState>>(
      '/AppConfig/ReplaceActive',
      { content }
    );
    return data.data;
  }

  // 获取当前客户端IP
  public static async getClientIP () {
    const { data } = await serverRequest.get<ServerResponse<{ ip: string; }>>(
      '/WebUIConfig/GetClientIP'
    );
    return data.data;
  }

  // 获取SSL证书状态
  public static async getSSLStatus () {
    const { data } = await serverRequest.get<ServerResponse<{
      enabled: boolean;
      certExists: boolean;
      keyExists: boolean;
      certContent: string;
      keyContent: string;
    }>>('/WebUIConfig/GetSSLStatus');
    return data.data;
  }

  // 保存SSL证书
  public static async saveSSLCert (cert: string, key: string) {
    const { data } = await serverRequest.post<ServerResponse<{ message: string; }>>(
      '/WebUIConfig/UploadSSLCert',
      { cert, key }
    );
    return data.data;
  }

  // 删除SSL证书
  public static async deleteSSLCert () {
    const { data } = await serverRequest.post<ServerResponse<{ message: string; }>>(
      '/WebUIConfig/DeleteSSLCert'
    );
    return data.data;
  }

  public static async getAppFileHash () {
    const { data } = await serverRequest.get<ServerResponse<{ hash: string; file: string; algorithm: string; }>>(
      '/base/GetAppFileHash'
    );
    return data.data;
  }
}
