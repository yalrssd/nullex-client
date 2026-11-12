import React, { useMemo, useState } from 'react'
import { toast } from 'sonner'
import { useTranslation } from 'react-i18next'
import {
  Activity,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Folder,
  FolderOpen,
  Globe,
  Info,
  Plus,
  RefreshCcw,
  Search,
  Send,
  X,
  Zap
} from 'lucide-react'
import { cn } from '@renderer/lib/utils'
import { mihomoChangeProxy, mihomoGroupDelay, mihomoProxyDelay } from '@renderer/utils/ipc'
import { useAppConfig } from '@renderer/hooks/use-app-config'
import { useGroups } from '@renderer/hooks/use-groups'
import { useProfileConfig } from '@renderer/hooks/use-profile-config'
import { extractLeadingFlagAndTitle } from './flag-utils'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from '@renderer/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@renderer/components/ui/popover'
import { calcTraffic } from '@renderer/utils/calc'
import dayjs from 'dayjs'

interface NullexServerPanelProps {
  onAddProfile: () => void
  onUpdateProfile: () => Promise<void>
  updatingProfile: boolean
}

function getLatencyColorClass(delay: number): string {
  if (delay === 0) return 'text-[#F87171]'
  if (delay < 0) return 'text-muted-foreground'
  if (delay < 100) return 'text-[#4ADE80]'
  if (delay < 300) return 'text-[#FACC15]'
  return 'text-[#F87171]'
}

function getLatestDelay(item: ControllerProxiesDetail | ControllerGroupDetail): number {
  if (item.history && item.history.length > 0) {
    return item.history[item.history.length - 1].delay
  }
  return -1
}

export const NullexServerPanel: React.FC<NullexServerPanelProps> = ({
  onAddProfile,
  onUpdateProfile,
  updatingProfile
}) => {
  const { t, i18n } = useTranslation()
  const isRu = i18n.language.toLowerCase().startsWith('ru')
  const { appConfig, patchAppConfig } = useAppConfig()
  const delayTestMode = appConfig?.delayTestMode ?? 'url'
  const { groups = [], mutate } = useGroups()
  const { profileConfig, changeCurrentProfile } = useProfileConfig()

  const [searchOpen, setSearchOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [testingGroup, setTestingGroup] = useState(false)
  const [testingNodeMap, setTestingNodeMap] = useState<Record<string, boolean>>({})
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({})

  const currentProfile = useMemo(() => {
    if (!profileConfig?.current || !profileConfig?.items) return null
    return profileConfig.items.find((item) => item.id === profileConfig.current) ?? null
  }, [profileConfig])

  const allProfiles = profileConfig?.items ?? []

  const selectableGroups = useMemo(
    () => groups.filter((g) => g.all && g.all.length > 0),
    [groups]
  )

  const primaryGroup = useMemo(
    () =>
      selectableGroups.find((g) => g.type === 'Selector') || selectableGroups[0] || groups[0],
    [selectableGroups, groups]
  )

  const isGroupExpanded = (groupName: string, index: number): boolean => {
    if (searchQuery.trim()) return true
    if (groupName in expandedGroups) return expandedGroups[groupName]
    return index === 0 || groupName === primaryGroup?.name
  }

  const toggleGroup = (groupName: string, currentExpanded: boolean): void => {
    setExpandedGroups((prev) => ({ ...prev, [groupName]: !currentExpanded }))
  }

  const handleSelectProxy = async (groupName: string, proxyName: string): Promise<void> => {
    try {
      await mihomoChangeProxy(groupName, proxyName)
      mutate()
    } catch (e) {
      toast.error(`${e}`)
    }
  }

  const handlePingAll = async (): Promise<void> => {
    if (!primaryGroup || testingGroup) return
    setTestingGroup(true)
    try {
      await mihomoGroupDelay(
        primaryGroup.name,
        primaryGroup.testUrl || 'http://cp.cloudflare.com'
      )
      mutate()
    } catch (e) {
      toast.error(`${e}`)
    } finally {
      setTestingGroup(false)
    }
  }

  const handlePingSingle = async (
    e: React.MouseEvent,
    proxyName: string,
    testUrl?: string
  ): Promise<void> => {
    e.stopPropagation()
    if (testingNodeMap[proxyName]) return
    setTestingNodeMap((prev) => ({ ...prev, [proxyName]: true }))
    try {
      await mihomoProxyDelay(proxyName, testUrl || 'http://cp.cloudflare.com')
      mutate()
    } catch {
      // ignore timeout
    } finally {
      setTestingNodeMap((prev) => ({ ...prev, [proxyName]: false }))
    }
  }

  const subscription = currentProfile?.extra
  const trafficUsed = (subscription?.upload ?? 0) + (subscription?.download ?? 0)
  const trafficTotal = subscription?.total ?? 0
  const trafficRemaining = trafficTotal > 0 ? Math.max(0, trafficTotal - trafficUsed) : 0
  const expireTimestamp = subscription?.expire ?? 0
  const expireLabel =
    expireTimestamp > 0 ? dayjs.unix(expireTimestamp).format('L') : t('pages.home.never')

  return (
    <aside className="w-[265px] shrink-0 h-full bg-background border-r border-border flex flex-col select-none z-20">
      {/* Server Header: Серверы + [Ping All] + [+] */}
      <div className="px-4 pt-3.5 pb-2 flex items-center justify-between gap-2 shrink-0">
        <h2 className="text-[19px] font-semibold tracking-tight text-foreground truncate">
          {isRu ? 'Серверы' : 'Servers'}
        </h2>
        <div className="flex items-center gap-0.5 shrink-0">
          <button
            type="button"
            title={t('sider.searchNode')}
            onClick={() => {
              setSearchOpen((v) => !v)
              if (searchOpen) setSearchQuery('')
            }}
            className={cn(
              'size-7 rounded-md inline-flex items-center justify-center transition-colors cursor-pointer',
              searchOpen
                ? 'bg-accent text-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-accent/60'
            )}
          >
            <Search className="size-3.5" />
          </button>
          <button
            type="button"
            title={t('proxies.delayTestMode')}
            onClick={async () => {
              await patchAppConfig({ delayTestMode: delayTestMode === 'tcp' ? 'url' : 'tcp' })
              mutate()
            }}
            className={cn(
              'h-6 px-1.5 rounded-md inline-flex items-center justify-center text-[10px] font-semibold uppercase tracking-wide border transition-colors cursor-pointer',
              delayTestMode === 'tcp'
                ? 'bg-emerald-500/15 border-emerald-500/40 text-emerald-400 hover:bg-emerald-500/25'
                : 'bg-secondary/60 border-border text-muted-foreground hover:text-foreground hover:bg-accent/60'
            )}
          >
            {delayTestMode === 'tcp' ? 'TCP' : 'URL'}
          </button>
          <button
            type="button"
            title={`${t('sider.delayTest')} (${delayTestMode.toUpperCase()})`}
            disabled={!primaryGroup || testingGroup}
            onClick={handlePingAll}
            className="size-7 rounded-md inline-flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors cursor-pointer disabled:opacity-45"
          >
            <Activity className={cn('size-4', testingGroup && 'animate-pulse text-foreground')} />
          </button>
          <button
            type="button"
            title={t('pages.profiles.addProfile')}
            onClick={onAddProfile}
            className="size-7 rounded-md inline-flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors cursor-pointer"
          >
            <Plus className="size-4" />
          </button>
        </div>
      </div>

      {/* Collapsible compact search input */}
      {searchOpen && (
        <div className="px-3 pb-2 shrink-0">
          <div className="relative flex items-center">
            <Search className="size-3.5 text-muted-foreground absolute left-2.5 pointer-events-none" />
            <input
              type="text"
              autoFocus
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={t('common.search') + '...'}
              className="w-full h-8 pl-8 pr-7 rounded-lg bg-secondary/70 border border-border text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:border-ring"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                className="absolute right-2 text-muted-foreground hover:text-foreground cursor-pointer"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
        </div>
      )}

      {/* Active Subscription / Profile Row (matches UPPERCASE SUBSCRIPTION + (i)) */}
      {currentProfile && (
        <div
          data-guide="home-profile-header"
          className="px-3 pb-1.5 flex items-center justify-between gap-1 shrink-0"
        >
          {allProfiles.length > 1 ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="min-w-0 flex-1 px-1.5 py-1 rounded-md flex items-center gap-1 text-left hover:bg-accent/60 transition-colors cursor-pointer"
                >
                  <span className="text-[11px] font-medium tracking-wider uppercase text-muted-foreground truncate">
                    {currentProfile.name}
                  </span>
                  <ChevronDown className="size-3 text-muted-foreground shrink-0" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56">
                {allProfiles.map((p) => (
                  <DropdownMenuItem
                    key={p.id}
                    onClick={() => changeCurrentProfile(p.id)}
                    className={cn(
                      'text-xs cursor-pointer',
                      p.id === currentProfile.id && 'bg-accent font-semibold'
                    )}
                  >
                    <span className="truncate">{p.name}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <div className="min-w-0 flex-1 px-1.5 py-1">
              <span className="text-[11px] font-medium tracking-wider uppercase text-muted-foreground truncate block">
                {currentProfile.name}
              </span>
            </div>
          )}

          <div className="flex items-center gap-0.5 shrink-0">
            {currentProfile.type === 'remote' && (
              <button
                type="button"
                disabled={updatingProfile}
                onClick={onUpdateProfile}
                title={t('common.update')}
                className="size-6 rounded-md inline-flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors cursor-pointer disabled:opacity-45"
              >
                <RefreshCcw className={cn('size-3', updatingProfile && 'animate-spin')} />
              </button>
            )}

            <Popover>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  title="Info"
                  className="size-6 rounded-md inline-flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-colors cursor-pointer"
                >
                  <Info className="size-3.5" />
                </button>
              </PopoverTrigger>
              <PopoverContent align="end" className="w-64 p-3 text-xs space-y-2">
                <div className="font-semibold text-foreground truncate">{currentProfile.name}</div>
                {subscription && (
                  <div className="space-y-1 text-muted-foreground">
                    <div className="flex justify-between">
                      <span>{t('pages.home.trafficRemaining')}</span>
                      <span className="text-foreground font-medium tabular-nums">
                        {trafficTotal > 0 ? calcTraffic(trafficRemaining) : t('pages.home.unlimited')}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span>{t('pages.home.expires')}</span>
                      <span className="text-foreground font-medium">{expireLabel}</span>
                    </div>
                  </div>
                )}
                {currentProfile.supportUrl && (
                  <button
                    type="button"
                    data-guide="home-support-link"
                    onClick={() => open(currentProfile.supportUrl)}
                    className="w-full h-7 rounded-md bg-secondary hover:bg-accent text-foreground font-medium inline-flex items-center justify-center gap-1.5 transition-colors cursor-pointer"
                  >
                    <Send className="size-3" />
                    <span>{t('pages.profiles.support')}</span>
                  </button>
                )}
              </PopoverContent>
            </Popover>
          </div>
        </div>
      )}

      {/* Server Group Tree List */}
      <div className="flex-1 min-h-0 overflow-y-auto px-2 pb-3 space-y-1">
        {selectableGroups.length === 0 ? (
          <div className="px-3 py-6 text-xs text-muted-foreground">
            {isRu ? 'Нет доступных серверов' : 'No servers yet'}
          </div>
        ) : (
          selectableGroups.map((group, groupIdx) => {
            const expanded = isGroupExpanded(group.name, groupIdx)
            const q = searchQuery.trim().toLowerCase()
            const items = (group.all || []).filter(
              (item) => !q || item.name.toLowerCase().includes(q)
            )
            if (q && items.length === 0) return null

            const isPrimary = group.name === primaryGroup?.name

            return (
              <div
                key={group.name}
                data-guide={isPrimary ? 'home-group-selector' : undefined}
                className="space-y-0.5"
              >
                {/* GroupRow header */}
                <button
                  type="button"
                  onClick={() => toggleGroup(group.name, expanded)}
                  className="w-full px-2.5 py-2 rounded-[8px] flex items-center gap-2 text-left hover:bg-accent/50 transition-colors cursor-pointer"
                >
                  {expanded ? (
                    <FolderOpen className="size-3.5 text-muted-foreground shrink-0" />
                  ) : (
                    <Folder className="size-3.5 text-muted-foreground shrink-0" />
                  )}
                  <span className="flex-1 min-w-0 text-[13px] font-medium text-foreground truncate">
                    {group.name}
                  </span>
                  {group.now && !expanded && (
                    <span className="text-[11px] text-muted-foreground truncate max-w-20">
                      {extractLeadingFlagAndTitle(group.now).title}
                    </span>
                  )}
                  {expanded ? (
                    <ChevronUp className="size-3.5 text-muted-foreground shrink-0" />
                  ) : (
                    <ChevronDown className="size-3.5 text-muted-foreground shrink-0" />
                  )}
                </button>

                {/* Group Proxies */}
                {expanded && (
                  <div className="pl-2 space-y-0.5">
                    {items.map((proxy) => {
                      const isSelected = group.now === proxy.name
                      const parsed = extractLeadingFlagAndTitle(proxy.name)
                      const delay = getLatestDelay(proxy)
                      const isPinging = Boolean(testingNodeMap[proxy.name])
                      const isAutoLike =
                        proxy.type === 'URLTest' ||
                        proxy.type === 'Fallback' ||
                        proxy.name.toLowerCase().includes('auto') ||
                        proxy.name.toLowerCase().includes('авто')
                      const subtitle =
                        !('all' in proxy) && proxy.serverDescription
                          ? proxy.serverDescription
                          : proxy.type

                      return (
                        <div
                          key={`${group.name}:${proxy.name}`}
                          onClick={() => handleSelectProxy(group.name, proxy.name)}
                          className={cn(
                            'relative group flex items-center gap-2.5 px-2.5 py-1.5 rounded-[8px] cursor-pointer transition-colors duration-150',
                            isSelected ? 'bg-accent' : 'hover:bg-accent/50'
                          )}
                        >
                          {/* Left Vertical Selection Indicator Bar */}
                          {isSelected && (
                            <span className="absolute -left-1 top-1/2 -translate-y-1/2 w-[3px] h-5 rounded-full bg-primary" />
                          )}

                          {/* Circular Flag / Icon Badge */}
                          <span className="size-[26px] rounded-full bg-card border border-border flex items-center justify-center shrink-0 overflow-hidden">
                            {parsed.flag ? (
                              <span className="flag-emoji text-[15px] leading-none">
                                {parsed.flag}
                              </span>
                            ) : isAutoLike ? (
                              <Zap className="size-3.5 text-muted-foreground" />
                            ) : (
                              <Globe className="size-3.5 text-muted-foreground" />
                            )}
                          </span>

                          {/* Server Name + Protocol Subtitle */}
                          <div className="min-w-0 flex-1">
                            <div className="text-[13px] text-foreground truncate leading-tight">
                              {parsed.title}
                            </div>
                            {subtitle && (
                              <div className="text-[11px] text-muted-foreground truncate leading-tight mt-0.5">
                                {subtitle}
                              </div>
                            )}
                          </div>

                          {/* Right Latency Indicator */}
                          <button
                            type="button"
                            onClick={(e) => handlePingSingle(e, proxy.name, group.testUrl)}
                            title={t('sider.delayTest')}
                            className={cn(
                              'shrink-0 px-1.5 py-0.5 rounded text-[11px] font-medium tabular-nums hover:bg-white/10 transition-colors cursor-pointer',
                              getLatencyColorClass(delay)
                            )}
                          >
                            {isPinging ? '…' : delay > 0 ? `${delay}ms` : '—'}
                          </button>

                          {isSelected && !('all' in proxy) && (
                            <ChevronRight className="size-3 text-muted-foreground/60 shrink-0 hidden" />
                          )}
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>
            )
          })
        )}
      </div>
    </aside>
  )
}

export default NullexServerPanel
