declare module 'electron' {
  export interface TitleBarOverlayOptions {
    color?: string
    symbolColor?: string
    height?: number
  }
}

declare namespace NodeJS {
  type Platform = 'aix' | 'darwin' | 'freebsd' | 'linux' | 'openbsd' | 'sunos' | 'win32'
}

declare namespace os {
  interface NetworkInterfaceInfo {
    address: string
    netmask: string
    family: 'IPv4' | 'IPv6'
    mac: string
    internal: boolean
    cidr: string | null
    scopeid?: number
  }
}

interface IpcRendererShim {
  invoke: (channel: string, ...args: unknown[]) => Promise<unknown>
  send: (channel: string, ...args: unknown[]) => void
  on: (channel: string, listener: (...args: unknown[]) => void) => void
  once: (channel: string, listener: (...args: unknown[]) => void) => void
  removeListener: (channel: string, listener: (...args: unknown[]) => void) => void
  removeAllListeners: (channel: string) => void
}

interface Window {
  electron: {
    ipcRenderer: IpcRendererShim
  }
  api: {
    platform: NodeJS.Platform
    webUtils: {
      getPathForFile: (file: File) => string
    }
  }
}
