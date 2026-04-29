const DEFAULT_TIMEOUT: TimeoutConfig = {
  baseTimeout: 10000,
  uploadSpeedKBps: 256,
  downloadSpeedKBps: 256,
  maxTimeout: 1800000,
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null

const readBoolean = (value: unknown, fallback = false): boolean =>
  typeof value === 'boolean' ? value : fallback

const readString = (value: unknown, fallback = ''): string =>
  typeof value === 'string' ? value : fallback

const readNumber = (value: unknown, fallback = 0): number =>
  typeof value === 'number' && Number.isFinite(value) ? value : fallback

const readConfigArray = <T>(value: unknown): T[] => Array.isArray(value) ? value as T[] : []

export const createDefaultOneBotConfig = (): OneBotConfig => ({
  network: {
    httpServers: [],
    httpClients: [],
    httpSseServers: [],
    websocketServers: [],
    websocketClients: [],
  },
  musicSignUrl: '',
  enableLocalFile2Url: false,
  parseMultMsg: true,
  imageDownloadProxy: '',
  timeout: { ...DEFAULT_TIMEOUT },
})

const normalizeTimeoutConfig = (value: unknown): TimeoutConfig => {
  if (!isRecord(value)) {
    return { ...DEFAULT_TIMEOUT }
  }

  return {
    baseTimeout: readNumber(value.baseTimeout, DEFAULT_TIMEOUT.baseTimeout),
    uploadSpeedKBps: readNumber(value.uploadSpeedKBps, DEFAULT_TIMEOUT.uploadSpeedKBps),
    downloadSpeedKBps: readNumber(value.downloadSpeedKBps, DEFAULT_TIMEOUT.downloadSpeedKBps),
    maxTimeout: readNumber(value.maxTimeout, DEFAULT_TIMEOUT.maxTimeout),
  }
}

const normalizeNetworkConfig = (value: unknown): NetworkConfig => {
  if (!isRecord(value)) {
    return createDefaultOneBotConfig().network
  }

  return {
    httpServers: readConfigArray<OneBotConfig['network']['httpServers'][0]>(value.httpServers),
    httpClients: readConfigArray<OneBotConfig['network']['httpClients'][0]>(value.httpClients),
    httpSseServers: readConfigArray<OneBotConfig['network']['httpSseServers'][0]>(value.httpSseServers),
    websocketServers: readConfigArray<OneBotConfig['network']['websocketServers'][0]>(value.websocketServers),
    websocketClients: readConfigArray<OneBotConfig['network']['websocketClients'][0]>(value.websocketClients),
  }
}

export const normalizeOneBotConfig = (value: unknown): OneBotConfig => {
  if (!isRecord(value)) {
    return createDefaultOneBotConfig()
  }

  const defaults = createDefaultOneBotConfig()

  return {
    network: normalizeNetworkConfig(value.network),
    musicSignUrl: readString(value.musicSignUrl, defaults.musicSignUrl),
    enableLocalFile2Url: readBoolean(value.enableLocalFile2Url, defaults.enableLocalFile2Url),
    parseMultMsg: readBoolean(value.parseMultMsg, defaults.parseMultMsg),
    imageDownloadProxy: readString(value.imageDownloadProxy, defaults.imageDownloadProxy),
    timeout: normalizeTimeoutConfig(value.timeout),
  }
}
