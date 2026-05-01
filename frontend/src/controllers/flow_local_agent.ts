import { serverRequest } from '@/utils/request';

export interface FlowLocalAgentConfigUpdate {
  enabled: boolean;
  baseUrl: string;
  token: string;
  deviceId: string;
  deviceName: string;
  autoConnect: boolean;
  allowedTools: string[];
  workspaceRoot: string;
  commandTimeoutSeconds: number;
  approvalPolicy: string;
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

  public static async getStatus () {
    const { data } = await serverRequest.get<ServerResponse<FlowLocalAgentStatus>>(
      '/FlowLocalAgent/GetStatus'
    );
    return data.data;
  }
}
