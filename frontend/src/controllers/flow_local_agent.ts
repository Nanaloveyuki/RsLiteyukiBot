import { serverRequest } from '@/utils/request';

export interface FlowLocalAgentConfigUpdate {
  enabled: boolean;
  baseUrl: string;
  deviceId: string;
  deviceName: string;
  autoConnect: boolean;
  allowedTools: string[];
  workspaceRoot: string;
  commandTimeoutSeconds: number;
  approvalPolicy: string;
}

export interface FlowLocalAgentTokenUpdate {
  token: string;
}

export interface FlowLocalAgentDeviceCodeStartInput {
  baseUrl?: string;
}

export interface FlowLocalAgentDeviceCodePollInput {
  baseUrl?: string;
  deviceCode: string;
}

export interface FlowLocalAgentConnectNowResult {
  requested: boolean;
}

export default class FlowLocalAgentManager {
  public static async getConfig () {
    const { data } = await serverRequest.get<ServerResponse<FlowLocalAgentConfig>>(
      '/FlowLocalAgent/GetConfig'
    );
    return data.data;
  }

  public static async setConfig (config: FlowLocalAgentConfigUpdate) {
    const { data } = await serverRequest.post<ServerResponse<FlowLocalAgentConfig>>(
      '/FlowLocalAgent/SetConfig',
      config
    );
    return data.data;
  }

  public static async setToken (payload: FlowLocalAgentTokenUpdate) {
    const { data } = await serverRequest.post<ServerResponse<FlowLocalAgentConfig>>(
      '/FlowLocalAgent/SetToken',
      payload
    );
    return data.data;
  }

  public static async startDeviceCode (payload: FlowLocalAgentDeviceCodeStartInput) {
    const { data } = await serverRequest.post<ServerResponse<FlowLocalAgentDeviceCode>>(
      '/FlowLocalAgent/Auth/DeviceCode/Start',
      payload
    );
    return data.data;
  }

  public static async pollDeviceCode (payload: FlowLocalAgentDeviceCodePollInput) {
    const { data } = await serverRequest.post<ServerResponse<FlowLocalAgentDeviceCodePollResult>>(
      '/FlowLocalAgent/Auth/DeviceCode/Poll',
      payload
    );
    return data.data;
  }

  public static async getStatus () {
    const { data } = await serverRequest.get<ServerResponse<FlowLocalAgentStatus>>(
      '/FlowLocalAgent/GetStatus'
    );
    return data.data;
  }

  public static async connectNow () {
    const { data } = await serverRequest.post<ServerResponse<FlowLocalAgentConnectNowResult>>(
      '/FlowLocalAgent/ConnectNow'
    );
    return data.data;
  }

  public static async disconnectNow () {
    const { data } = await serverRequest.post<ServerResponse<FlowLocalAgentConnectNowResult>>(
      '/FlowLocalAgent/DisconnectNow'
    );
    return data.data;
  }

  public static async getLogs () {
    const { data } = await serverRequest.get<ServerResponse<FlowLocalAgentLogsPayload>>(
      '/FlowLocalAgent/GetLogs'
    );
    return data.data;
  }
}
