import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

type IpcListener = (event: unknown, ...args: unknown[]) => void

const listenersMap = new Map<string, Set<IpcListener>>()
const tauriUnlistens = new Map<string, Promise<UnlistenFn>>()
const droppedFiles = new Map<string, File>()
let dropCounter = 0

function ensureChannelSubscribed(channel: string): void {
  if (tauriUnlistens.has(channel)) return
  const promise = listen<unknown[]>(`ipc-${channel}`, (ev) => {
    const args = Array.isArray(ev.payload) ? ev.payload : ev.payload !== undefined ? [ev.payload] : []
    const set = listenersMap.get(channel)
    if (!set) return
    for (const cb of [...set]) {
      try {
        cb({}, ...args)
      } catch (err) {
        console.error(`[TauriShim] listener error for ${channel}:`, err)
      }
    }
  })
  tauriUnlistens.set(channel, promise)
}

function installGlobalThemeStyle(css: string): void {
  let styleEl = document.getElementById('nullex-custom-theme') as HTMLStyleElement | null
  if (!styleEl) {
    styleEl = document.createElement('style')
    styleEl.id = 'nullex-custom-theme'
    document.head.appendChild(styleEl)
  }
  styleEl.textContent = css
}

// Listen for custom CSS pushes from Rust backend
listen<{ css: string }>('nullex-apply-theme-css', (ev) => {
  installGlobalThemeStyle(ev.payload?.css || '')
}).catch(() => {})

// Bridge Electron `.app-drag` / `.app-nodrag` header dragging for WebView2
if (typeof document !== 'undefined') {
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return
    const target = e.target as HTMLElement | null
    if (!target) return
    if (
      target.closest('.app-nodrag') ||
      target.closest('button, a, input, select, textarea, [role="button"], [contenteditable="true"]')
    ) {
      return
    }
    if (target.closest('.app-drag')) {
      getCurrentWindow()
        .startDragging()
        .catch(() => {})
    }
  })

  document.addEventListener('dblclick', (e) => {
    const target = e.target as HTMLElement | null
    if (!target) return
    if (
      target.closest('.app-nodrag') ||
      target.closest('button, a, input, select, textarea, [role="button"]')
    ) {
      return
    }
    if (target.closest('.app-drag')) {
      getCurrentWindow()
        .toggleMaximize()
        .catch(() => {})
    }
  })
}

const ipcRendererShim = {
  invoke: async (channel: string, ...args: unknown[]): Promise<unknown> => {
    if (channel === 'readTextFile' && typeof args[0] === 'string' && args[0].startsWith('tauri-drop-file://')) {
      const file = droppedFiles.get(args[0])
      droppedFiles.delete(args[0])
      if (file) {
        return await file.text()
      }
    }
    try {
      const res = await invoke('ipc_invoke', { channel, args })
      return res === null ? undefined : res
    } catch (e) {
      return { invokeError: typeof e === 'string' ? e : e instanceof Error ? e.message : String(e) }
    }
  },
  send: (channel: string, ...args: unknown[]): void => {
    invoke('ipc_send', { channel, args }).catch(() => {})
  },
  on: (channel: string, listener: IpcListener): void => {
    let set = listenersMap.get(channel)
    if (!set) {
      set = new Set()
      listenersMap.set(channel, set)
    }
    set.add(listener)
    ensureChannelSubscribed(channel)
  },
  once: (channel: string, listener: IpcListener): void => {
    const wrapped: IpcListener = (ev, ...args) => {
      ipcRendererShim.removeListener(channel, wrapped)
      listener(ev, ...args)
    }
    ipcRendererShim.on(channel, wrapped)
  },
  removeListener: (channel: string, listener: IpcListener): void => {
    listenersMap.get(channel)?.delete(listener)
  },
  removeAllListeners: (channel: string): void => {
    listenersMap.delete(channel)
  }
}

if (!window.electron) {
  ;(window as unknown as Record<string, unknown>).electron = {
    ipcRenderer: ipcRendererShim
  }
}

if (!window.api) {
  ;(window as unknown as Record<string, unknown>).api = {
    platform: 'win32',
    webUtils: {
      getPathForFile: (file: File): string => {
        const nativePath = (file as unknown as { path?: string }).path
        if (nativePath) return nativePath
        const key = `tauri-drop-file://${++dropCounter}`
        droppedFiles.set(key, file)
        return key
      }
    }
  }
}

export {}
