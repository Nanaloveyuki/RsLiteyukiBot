/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_BASE?: string
  readonly VITE_DEBUG_BACKEND_URL?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

interface LiteyukiDesktopClosePayload {
  closeToTrayDefault: boolean
}

interface LiteyukiDesktopBridge {
  onCloseRequested: (
    listener: (payload: LiteyukiDesktopClosePayload) => void
  ) => () => void
  closeToBackground: (alwaysRemember: boolean) => Promise<void>
  exitApp: (alwaysRemember: boolean) => Promise<void>
  cancelClose: () => Promise<void>
}

interface Window {
  __LITEYUKI_DESKTOP__?: LiteyukiDesktopBridge
}
