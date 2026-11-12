import React, { useMemo, useState } from 'react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle
} from '@renderer/components/ui/dialog'
import { useAppConfig } from '@renderer/hooks/use-app-config'
import { useGroups } from '@renderer/hooks/use-groups'
import { mihomoChangeProxy, mihomoGroupDelay } from '@renderer/utils/ipc'
import { Search, Zap, Check, Activity, Globe, X } from 'lucide-react'
import { toast } from 'sonner'
import { cn } from '@renderer/lib/utils'
import { extractLeadingFlagAndTitle } from './flag-utils'

interface QuickServerModalProps {
  open: boolean
  onClose: () => void
}

export const QuickServerModal: React.FC<QuickServerModalProps> = ({ open, onClose }) => {
  const { appConfig, patchAppConfig } = useAppConfig()
  const delayTestMode = appConfig?.delayTestMode ?? 'url'
  const { groups = [], mutate } = useGroups()
  const [search, setSearch] = useState('')
  const [testing, setTesting] = useState(false)
  const [selectedGroupName, setSelectedGroupName] = useState<string | null>(null)

  const selectableGroups = useMemo(
    () => groups.filter((g) => g.all && g.all.length > 0),
    [groups]
  )

  const primaryGroup = useMemo(() => {
    if (selectedGroupName) {
      const found = selectableGroups.find((g) => g.name === selectedGroupName)
      if (found) return found
    }
    return selectableGroups[0] || groups[0]
  }, [selectableGroups, groups, selectedGroupName])

  const handleSelectProxy = async (proxyName: string): Promise<void> => {
    if (!primaryGroup) return
    try {
      await mihomoChangeProxy(primaryGroup.name, proxyName)
      mutate()
      onClose()
    } catch (e) {
      toast.error(`${e}`)
    }
  }

  const handleTestLatency = async (): Promise<void> => {
    if (!primaryGroup) return
    setTesting(true)
    try {
      await mihomoGroupDelay(primaryGroup.name, primaryGroup.testUrl || 'http://cp.cloudflare.com')
      mutate()
    } catch (e) {
      toast.error(`${e}`)
    } finally {
      setTesting(false)
    }
  }

  const allProxies = primaryGroup?.all || []
  const filtered = allProxies.filter((p) =>
    p.name.toLowerCase().includes(search.toLowerCase())
  )

  const getDelay = (p: { history?: ControllerProxiesHistory[] }): number => {
    if (p.history && p.history.length > 0) {
      return p.history[p.history.length - 1].delay
    }
    return -1
  }

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <DialogContent
        showCloseButton={false}
        className="max-w-lg bg-card border-border text-foreground shadow-2xl p-0 gap-0 overflow-hidden rounded-2xl"
      >
        <DialogHeader className="p-4 border-b border-border flex flex-row items-center justify-between space-y-0">
          <div className="flex items-center gap-2 min-w-0">
            <Globe className="size-4 text-primary shrink-0" />
            <DialogTitle className="text-sm font-semibold text-foreground tracking-tight truncate">
              {primaryGroup ? primaryGroup.name : 'Servers'}
            </DialogTitle>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-accent transition-colors cursor-pointer shrink-0"
          >
            <X className="size-4" />
          </button>
        </DialogHeader>

        {selectableGroups.length > 1 && (
          <div className="px-3 pt-2.5 pb-1.5 flex items-center gap-1.5 overflow-x-auto no-scrollbar border-b border-border">
            {selectableGroups.map((g) => {
              const active = primaryGroup?.name === g.name
              return (
                <button
                  key={g.name}
                  type="button"
                  onClick={() => setSelectedGroupName(g.name)}
                  className={cn(
                    'px-2.5 py-1 rounded-md text-xs font-medium transition-colors shrink-0 cursor-pointer',
                    active
                      ? 'bg-accent text-foreground'
                      : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
                  )}
                >
                  {g.name}
                </button>
              )
            })}
          </div>
        )}

        <div className="p-3 border-b border-border flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="size-3.5 text-muted-foreground absolute left-3 top-1/2 -translate-y-1/2 pointer-events-none" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search servers..."
              autoFocus
              className="w-full h-9 pl-8 pr-3 rounded-lg bg-secondary/70 border border-border text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-ring"
            />
          </div>
          <button
            type="button"
            onClick={async () => {
              await patchAppConfig({ delayTestMode: delayTestMode === 'tcp' ? 'url' : 'tcp' })
              mutate()
            }}
            className={cn(
              'h-9 px-2.5 rounded-lg border text-[11px] font-semibold uppercase tracking-wide transition-colors cursor-pointer',
              delayTestMode === 'tcp'
                ? 'bg-emerald-500/15 border-emerald-500/40 text-emerald-400 hover:bg-emerald-500/25'
                : 'bg-secondary hover:bg-accent border-border text-muted-foreground hover:text-foreground'
            )}
          >
            {delayTestMode === 'tcp' ? 'TCP' : 'URL'}
          </button>
          <button
            type="button"
            onClick={handleTestLatency}
            disabled={testing || !primaryGroup}
            className="h-9 px-3 rounded-lg bg-secondary hover:bg-accent border border-border text-xs font-medium text-foreground flex items-center gap-1.5 transition-colors cursor-pointer disabled:opacity-50"
          >
            <Zap className={cn('size-3.5', testing && 'animate-spin')} />
            <span>Ping</span>
          </button>
        </div>

        <div className="max-h-80 overflow-y-auto p-2 space-y-0.5">
          {filtered.length === 0 ? (
            <div className="py-10 text-center text-xs text-muted-foreground">No servers found</div>
          ) : (
            filtered.map((proxy) => {
              const isSelected = primaryGroup?.now === proxy.name
              const delay = getDelay(proxy)
              const parsed = extractLeadingFlagAndTitle(proxy.name)
              return (
                <button
                  key={proxy.name}
                  type="button"
                  onClick={() => handleSelectProxy(proxy.name)}
                  className={cn(
                    'relative w-full flex items-center justify-between px-3 py-2 rounded-lg text-left transition-colors cursor-pointer',
                    isSelected ? 'bg-accent' : 'hover:bg-accent/50'
                  )}
                >
                  <div className="flex items-center gap-2.5 min-w-0 pr-3">
                    <span className="size-6 rounded-full bg-background border border-border flex items-center justify-center shrink-0">
                      {parsed.flag ? (
                        <span className="flag-emoji text-sm leading-none">{parsed.flag}</span>
                      ) : (
                        <Globe className="size-3 text-muted-foreground" />
                      )}
                    </span>
                    <div className="min-w-0">
                      <div className="text-sm text-foreground truncate">{parsed.title}</div>
                      <div className="text-[11px] text-muted-foreground">{proxy.type}</div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <span
                      className={cn(
                        'text-xs font-medium tabular-nums inline-flex items-center gap-1',
                        delay === 0
                          ? 'text-[#F87171]'
                          : delay > 0 && delay < 100
                            ? 'text-[#4ADE80]'
                            : delay >= 100 && delay < 300
                              ? 'text-[#FACC15]'
                              : delay >= 300
                                ? 'text-[#F87171]'
                                : 'text-muted-foreground'
                      )}
                    >
                      <Activity className="size-3" />
                      {delay > 0 ? `${delay}ms` : '—'}
                    </span>
                    {isSelected && <Check className="size-4 text-foreground" />}
                  </div>
                </button>
              )
            })
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}

export default QuickServerModal
