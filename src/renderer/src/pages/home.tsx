import { toast } from 'sonner'
import BasePage from '@renderer/components/base/base-page'
import { useAppConfig } from '@renderer/hooks/use-app-config'
import { useControledMihomoConfig } from '@renderer/hooks/use-controled-mihomo-config'
import { useProfileConfig } from '@renderer/hooks/use-profile-config'
import { useGroups } from '@renderer/hooks/use-groups'
import { triggerSysProxy, updateTrayIcon, mihomoHotReloadConfig, isRunningAsAdmin, restartAsAdmin, checkTunStatus, runTunDiagnostics } from '@renderer/utils/ipc'
import { useTranslation } from 'react-i18next'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import dayjs from 'dayjs'
import {
  PlusCircle,
  Globe,
  CalendarClock,
  CreditCard,
  AlertTriangle,
  ChevronsUpDown,
  Sparkles,
  PanelLeftClose,
  PanelLeftOpen
} from 'lucide-react'
import EditInfoModal from '@renderer/components/profiles/edit-info-modal'
import { calcTraffic } from '@renderer/utils/calc'
import { useTrafficStore } from '@renderer/store/traffic-store'
import { reopenCoreError, useCoreLifecycleStore } from '@renderer/store/core-lifecycle-store'
import { NullexGlobe } from '@renderer/components/globe/NullexGlobe'
import { NullexStarfield, type GlobeRotationState } from '@renderer/components/globe/NullexStarfield'
import { NullexShaderCanvas } from '@renderer/components/globe/NullexShaderCanvas'
import OffProxyTunSelector, { ConnectionMode } from '@renderer/components/home/OffProxyTunSelector'
import QuickServerModal from '@renderer/components/home/QuickServerModal'
import NullexServerPanel from '@renderer/components/home/NullexServerPanel'
import { extractLeadingFlagAndTitle } from '@renderer/components/home/flag-utils'
import { cn } from '@renderer/lib/utils'
import nullexIcon from '@renderer/assets/nullex-appicon.svg'

export type HomeBgPreset = 'globe-galaxy' | 'globe' | 'galaxy' | 'color-bends' | 'simple-gradient'

const EXPIRY_WARNING_DAYS = 3
const BG_PRESET_STORAGE_KEY = 'nullex_home_bg_preset'

let connectionStartTime: number | null = null

const ConnectedTimer = memo(({ active }: { active: boolean }) => {
  const [elapsed, setElapsed] = useState(0)

  useEffect(() => {
    if (!active) {
      connectionStartTime = null
      setElapsed(0)
      return undefined
    }

    if (connectionStartTime === null) {
      connectionStartTime = Date.now()
    }

    const updateElapsed = (): void => {
      setElapsed(Math.floor((Date.now() - connectionStartTime!) / 1000))
    }
    updateElapsed()
    const interval = setInterval(updateElapsed, 1000)
    return () => clearInterval(interval)
  }, [active])

  const hours = Math.floor(elapsed / 3600)
  const minutes = Math.floor((elapsed % 3600) / 60)
  const seconds = elapsed % 60
  return (
    <span>
      {String(hours).padStart(2, '0')}:{String(minutes).padStart(2, '0')}:
      {String(seconds).padStart(2, '0')}
    </span>
  )
})
ConnectedTimer.displayName = 'ConnectedTimer'

const Home: React.FC = () => {
  const { t, i18n } = useTranslation()
  const isRu = i18n.language.toLowerCase().startsWith('ru')
  const { appConfig, patchAppConfig } = useAppConfig()
  const { sysProxy, proxyMode = false, onlyActiveDevice = false } = appConfig || {}
  const { enable: writeSysProxy = true, mode: sysProxyMode } = sysProxy || {}
  const { controledMihomoConfig, patchControledMihomoConfig } = useControledMihomoConfig()
  const { tun } = controledMihomoConfig || {}
  const { 'mixed-port': mixedPort } = controledMihomoConfig || {}
  const sysProxyDisabled = mixedPort == 0

  const { profileConfig, addProfileItem } = useProfileConfig()
  const { groups } = useGroups()
  const hasProfiles = (profileConfig?.items?.length ?? 0) > 0

  const [showEditModal, setShowEditModal] = useState(false)
  const [editingItem, setEditingItem] = useState<ProfileItem | null>(null)
  const [updating, setUpdating] = useState(false)
  const [showServerModal, setShowServerModal] = useState(false)
  const [serverPanelOpen, setServerPanelOpen] = useState(true)
  const [bgPreset, setBgPreset] = useState<HomeBgPreset>(() => {
    const saved = window.localStorage.getItem(BG_PRESET_STORAGE_KEY) as HomeBgPreset | null
    return saved || 'globe-galaxy'
  })

  const globeRotationRef = useRef<GlobeRotationState>({ phi: 0.45, theta: -0.2 })

  const handleSelectBgPreset = (preset: HomeBgPreset): void => {
    setBgPreset(preset)
    window.localStorage.setItem(BG_PRESET_STORAGE_KEY, preset)
  }

  const handleAddProfile = (): void => {
    const newProfile: ProfileItem = {
      id: '',
      name: '',
      type: 'remote',
      url: '',
      useProxy: false,
      autoUpdate: true
    }
    setEditingItem(newProfile)
    setShowEditModal(true)
  }

  const trafficInfo = useTrafficStore((s) => s.traffic)

  const [loading, setLoading] = useState(false)
  const [loadingDirection, setLoadingDirection] = useState<'connecting' | 'disconnecting'>(
    'connecting'
  )

  const coreProblem = useCoreLifecycleStore((s) => s.coreState.status === 'error')
  const isTunOn = !coreProblem && (tun?.enable ?? false)
  const isProxyOn = !coreProblem && proxyMode
  const connectionMode: ConnectionMode = isTunOn ? 'tun' : isProxyOn ? 'proxy' : 'off'
  const isConnected = connectionMode !== 'off'

  const statusText = loading
    ? loadingDirection === 'connecting'
      ? t('pages.home.connecting')
      : t('pages.home.disconnecting')
    : coreProblem
      ? t('pages.home.coreProblem')
      : isConnected
        ? t('pages.home.connected')
        : t('pages.home.disconnected')

  const showConnectedTimer = !loading && isConnected

  const currentProfile = useMemo(() => {
    if (!profileConfig?.current || !profileConfig?.items) return null
    return profileConfig.items.find((item) => item.id === profileConfig.current) ?? null
  }, [profileConfig])

  const handleUpdateProfile = async (): Promise<void> => {
    if (!currentProfile || updating) return
    setUpdating(true)
    try {
      await addProfileItem(currentProfile)
    } catch (e) {
      toast.error(`${e}`)
    } finally {
      setUpdating(false)
    }
  }

  const subscription = currentProfile?.extra
  const expireTimestamp = subscription?.expire ?? 0
  const expireDate =
    expireTimestamp > 0 ? dayjs.unix(expireTimestamp).format('L') : t('pages.home.never')

  const [expiryTick, setExpiryTick] = useState(0)
  useEffect(() => {
    if (expireTimestamp <= 0) return undefined
    const interval = setInterval(() => setExpiryTick((n) => n + 1), 60_000)
    return () => clearInterval(interval)
  }, [expireTimestamp])

  const { daysRemaining, isExpired } = useMemo(() => {
    if (expireTimestamp <= 0) return { daysRemaining: 0, isExpired: false }
    const expiresAt = dayjs.unix(expireTimestamp)
    return {
      daysRemaining: Math.max(0, expiresAt.diff(dayjs(), 'day')),
      isExpired: expiresAt.isBefore(dayjs())
    }
  }, [expireTimestamp, expiryTick])

  const showExpiryNotice = expireTimestamp > 0 && daysRemaining <= EXPIRY_WARNING_DAYS
  const renewAction =
    currentProfile?.homeName && currentProfile?.home
      ? { url: currentProfile.home, label: currentProfile.homeName }
      : currentProfile?.supportUrl
        ? { url: currentProfile.supportUrl, label: t('pages.home.renewSubscription') }
        : null
  const expiryTitle = isExpired
    ? t('pages.home.subscriptionExpired')
    : daysRemaining === 0
      ? t('pages.home.subscriptionExpiringToday')
      : t('pages.home.subscriptionExpiring', { count: daysRemaining })

  const firstGroup = useMemo(
    () => groups?.find((g) => g.type === 'Selector' && g.all && g.all.length > 0) ||
      groups?.find((g) => g.all && g.all.length > 0) ||
      groups?.[0],
    [groups]
  )

  const activeProxyDelay = useMemo(() => {
    if (!firstGroup?.now || !firstGroup?.all) return -1
    const match = firstGroup.all.find((p) => p.name === firstGroup.now)
    if (match?.history && match.history.length > 0) {
      return match.history[match.history.length - 1].delay
    }
    return -1
  }, [firstGroup])

  const handleModeChange = async (targetMode: ConnectionMode): Promise<void> => {
    if (coreProblem && targetMode !== 'off') {
      reopenCoreError()
      return
    }
    if (targetMode === connectionMode || loading) return

    setLoading(true)
    setLoadingDirection(targetMode === 'off' ? 'disconnecting' : 'connecting')

    try {
      const tunWasEnabled = tun?.enable ?? false
      const proxyWasEnabled = proxyMode

      if (targetMode === 'off') {
        if (tunWasEnabled) {
          await patchControledMihomoConfig({ tun: { enable: false } })
        }
        if (proxyWasEnabled) {
          if (writeSysProxy) {
            await triggerSysProxy(false, onlyActiveDevice)
          }
          await patchAppConfig({ proxyMode: false })
        }
        if (tunWasEnabled || proxyWasEnabled) {
          await mihomoHotReloadConfig()
        }
      } else if (targetMode === 'proxy') {
        if (writeSysProxy && sysProxyMode === 'manual' && sysProxyDisabled) {
          toast.error('Mixed port is disabled (0). Enable mixed-port in settings to use Proxy mode.')
          return
        }
        if (tunWasEnabled) {
          await patchControledMihomoConfig({ tun: { enable: false } })
        }
        await patchAppConfig({ mainSwitchMode: 'sysproxy', proxyMode: true })
        await mihomoHotReloadConfig()
        if (writeSysProxy) {
          await triggerSysProxy(true, onlyActiveDevice)
        }
      } else if (targetMode === 'tun') {
        // Check admin rights before enabling TUN (WinTun driver requires elevation)
        const isAdmin = await isRunningAsAdmin().catch(() => false)
        if (!isAdmin) {
          toast.error(
            isRu
              ? 'TUN требует прав администратора. Перезапустить от имени администратора?'
              : 'TUN mode requires Administrator rights. Restart as Administrator?',
            {
              action: {
                label: isRu ? 'Перезапустить' : 'Restart',
                onClick: () => restartAsAdmin().catch((e: unknown) => toast.error(String(e)))
              },
              duration: 8000
            }
          )
          return
        }
        if (proxyWasEnabled) {
          if (writeSysProxy) {
            await triggerSysProxy(false, onlyActiveDevice)
          }
          await patchAppConfig({ proxyMode: false })
        }
        await patchControledMihomoConfig({ tun: { enable: true }, dns: { enable: true } })
        await mihomoHotReloadConfig()

        // Verify TUN adapter status (wait up to 3s for adapter creation)
        await new Promise((resolve) => setTimeout(resolve, 1500))
        let status = await checkTunStatus().catch(() => null)
        if (status && !status.isUp) {
          await new Promise((resolve) => setTimeout(resolve, 1500))
          status = await checkTunStatus().catch(() => null)
        }

        if (status && !status.isUp) {
          // TUN failed to start: roll back to Proxy or Off
          await patchControledMihomoConfig({ tun: { enable: false } })
          await patchAppConfig({ mainSwitchMode: 'sysproxy', proxyMode: proxyWasEnabled })
          await mihomoHotReloadConfig()
          if (proxyWasEnabled && writeSysProxy) {
            await triggerSysProxy(true, onlyActiveDevice)
          }

          const errMsg = status.errorMessage || (isRu ? 'TUN адаптер не запустился' : 'TUN adapter failed to start')
          const suggestion = status.suggestion || ''
          toast.error(`${errMsg}${suggestion ? ` (${suggestion})` : ''}`, {
            duration: 9000,
            action: {
              label: isRu ? 'Диагностика' : 'Diagnostics',
              onClick: () => {
                runTunDiagnostics().then((diag) => {
                  navigator.clipboard.writeText(diag.summary)
                  toast.success(isRu ? 'Отчёт скопирован в буфер обмена' : 'Diagnostic report copied to clipboard')
                })
              }
            }
          })
          return
        }

        await patchAppConfig({ mainSwitchMode: 'tun' })
      }

      window.electron.ipcRenderer.send('updateFloatingWindow')
      window.electron.ipcRenderer.send('updateTrayMenu')
      await updateTrayIcon()
    } catch (e) {
      toast.error(`${e}`)
    } finally {
      setLoading(false)
    }
  }

  const activeServerName = firstGroup?.now || undefined
  const parsedServer = activeServerName ? extractLeadingFlagAndTitle(activeServerName) : null

  const globeAppearance = useMemo(() => {
    if (coreProblem) {
      return { dotColor: '#E5E5E5', accentColor: '#FF6466', active: false }
    }
    if (connectionMode === 'tun') {
      return { dotColor: '#E5E5E5', accentColor: '#60A5FA', active: true }
    }
    if (connectionMode === 'proxy') {
      return { dotColor: '#E5E5E5', accentColor: '#4ADE80', active: true }
    }
    return { dotColor: '#E5E5E5', accentColor: '#E5E5E5', active: false }
  }, [coreProblem, connectionMode])

  const bgPresets: { id: HomeBgPreset; label: string }[] = [
    { id: 'globe-galaxy', label: isRu ? 'Глобус' : 'Globe' },
    { id: 'galaxy', label: isRu ? 'Космос' : 'Galaxy' },
    { id: 'color-bends', label: isRu ? 'Потоки' : 'Color Bends' },
    { id: 'simple-gradient', label: isRu ? 'Сияние' : 'Aurora' },
    { id: 'globe', label: isRu ? 'Сфера' : 'Sphere' }
  ]

  const renderStageBackdrop = (): React.ReactNode => {
    switch (bgPreset) {
      case 'galaxy':
        return <NullexShaderCanvas preset="galaxy" />
      case 'color-bends':
        return <NullexShaderCanvas preset="color-bends" />
      case 'simple-gradient':
        return <NullexShaderCanvas preset="simple-gradient" />
      case 'globe':
        return (
          <>
            <NullexStarfield count={100} rotationRef={globeRotationRef} />
            <div className="absolute inset-0 flex items-center justify-center">
              <div className="w-[min(74vh,560px)] h-[min(74vh,560px)] max-w-full max-h-full">
                <NullexGlobe
                  activeServerName={activeServerName}
                  dotColor={globeAppearance.dotColor}
                  accentColor={globeAppearance.accentColor}
                  active={globeAppearance.active}
                  rotationRef={globeRotationRef}
                />
              </div>
            </div>
          </>
        )
      case 'globe-galaxy':
      default:
        return (
          <>
            <NullexShaderCanvas preset="galaxy" className="opacity-85" />
            <div className="absolute inset-0 flex items-center justify-center">
              <div className="w-[min(70vh,520px)] h-[min(70vh,520px)] max-w-full max-h-full">
                <NullexGlobe
                  activeServerName={activeServerName}
                  dotColor={globeAppearance.dotColor}
                  accentColor={globeAppearance.accentColor}
                  active={globeAppearance.active}
                  rotationRef={globeRotationRef}
                />
              </div>
            </div>
          </>
        )
    }
  }

  return (
    <BasePage>
      <div className="relative h-full w-full overflow-hidden flex bg-background text-foreground">
        {!hasProfiles ? (
          /* Clean Neutral Empty / Onboarding State */
          <div className="relative flex-1 h-full w-full flex flex-col items-center justify-center overflow-hidden">
            <NullexShaderCanvas preset="galaxy" className="opacity-80" />
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center opacity-50">
              <div className="w-[min(65vh,460px)] h-[min(65vh,460px)]">
                <NullexGlobe dotColor="#E5E5E5" accentColor="#E5E5E5" active={false} />
              </div>
            </div>

            <div className="relative z-10 flex flex-col items-center gap-4 max-w-sm rounded-2xl border border-border bg-card/90 backdrop-blur-xl p-8 shadow-2xl mx-4">
              <img src={nullexIcon} alt="Nullex" className="size-14 rounded-xl" />
              <div className="text-center space-y-1.5">
                <h2 className="text-lg font-semibold text-foreground tracking-tight">
                  {t('pages.profiles.emptyTitle')}
                </h2>
                <p className="text-xs text-muted-foreground text-balance leading-relaxed">
                  {t('pages.profiles.emptyDescription')}
                </p>
              </div>
              <button
                type="button"
                onClick={handleAddProfile}
                data-guide="home-add-profile-btn"
                className="mt-1 inline-flex items-center gap-2 rounded-xl bg-primary text-primary-foreground hover:bg-primary/90 px-5 py-2.5 font-medium text-xs transition-colors shadow-sm cursor-pointer"
              >
                <PlusCircle className="size-4" />
                <span>{t('pages.profiles.addProfile')}</span>
              </button>
            </div>
          </div>
        ) : (
          <>
            {/* Left Docked Server Tree Panel */}
            {serverPanelOpen && (
              <NullexServerPanel
                onAddProfile={handleAddProfile}
                onUpdateProfile={handleUpdateProfile}
                updatingProfile={updating}
              />
            )}

            {/* Right Main Stage */}
            <div className="relative flex-1 min-w-0 h-full overflow-hidden bg-background flex flex-col justify-between">
              {/* Background: Shader +/or Monochrome Dotted Globe */}
              <div className="absolute inset-0 z-0">{renderStageBackdrop()}</div>

              {/* Subtle Top Controls Bar on Stage */}
              <div className="relative z-10 flex items-start justify-between gap-3 p-3 pointer-events-none">
                <div className="flex items-center gap-2 pointer-events-auto min-w-0">
                  <button
                    type="button"
                    onClick={() => setServerPanelOpen((v) => !v)}
                    title={serverPanelOpen ? t('common.hideSidebar') : t('common.toggleSidebar')}
                    className="size-8 rounded-lg bg-[#171717]/75 hover:bg-[#262626] backdrop-blur-md border border-white/10 inline-flex items-center justify-center text-muted-foreground hover:text-foreground transition-colors cursor-pointer shrink-0"
                  >
                    {serverPanelOpen ? (
                      <PanelLeftClose className="size-3.5" />
                    ) : (
                      <PanelLeftOpen className="size-3.5" />
                    )}
                  </button>

                  {currentProfile?.announce && (
                    <div
                      data-guide="home-profile-announce"
                      title={currentProfile.announce}
                      className="max-w-xs md:max-w-md rounded-lg bg-[#171717]/75 backdrop-blur-md border border-white/10 px-3 py-1.5 text-xs text-muted-foreground truncate"
                    >
                      {currentProfile.announce}
                    </div>
                  )}

                  {subscription && showExpiryNotice && (
                    <div className="rounded-lg bg-destructive/15 border border-destructive/30 backdrop-blur-md px-3 py-1.5 flex items-center gap-2 text-xs text-foreground">
                      <CalendarClock className="size-3.5 text-[#FF6466] shrink-0" />
                      <span className="truncate">{expiryTitle}</span>
                      {renewAction ? (
                        <button
                          type="button"
                          onClick={() => open(renewAction.url)}
                          className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-destructive/25 hover:bg-destructive/35 text-[11px] font-medium cursor-pointer shrink-0"
                        >
                          <CreditCard className="size-3" />
                          {renewAction.label}
                        </button>
                      ) : (
                        <span className="text-muted-foreground">{expireDate}</span>
                      )}
                    </div>
                  )}
                </div>

                {/* Background Style Switcher */}
                <div className="pointer-events-auto shrink-0 inline-flex items-center gap-0.5 rounded-lg bg-[#171717]/75 backdrop-blur-md border border-white/10 p-0.5">
                  <Sparkles className="size-3 text-muted-foreground mx-1.5 shrink-0" />
                  {bgPresets.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => handleSelectBgPreset(item.id)}
                      className={cn(
                        'px-2 py-1 rounded-md text-[11px] transition-colors cursor-pointer',
                        bgPreset === item.id
                          ? 'bg-[#262626] text-foreground font-medium'
                          : 'text-muted-foreground hover:text-foreground'
                      )}
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
              </div>

              {/* Bottom-Centered Dock (max-w-[340px], matching desktop_screen.png) */}
              <div className="relative z-20 w-full flex flex-col items-center px-4 pb-10 pointer-events-none">
                <div className="w-full max-w-[340px] flex flex-col gap-2 pointer-events-auto">
                  {/* Core Error Banner if core failed */}
                  {coreProblem && (
                    <button
                      type="button"
                      onClick={reopenCoreError}
                      className="w-full rounded-[14px] bg-[#171717]/90 border border-[#FF6466]/40 px-3.5 py-2 flex items-center gap-2 text-xs text-[#FF6466] hover:bg-[#262626] transition-colors cursor-pointer"
                    >
                      <AlertTriangle className="size-3.5 shrink-0" />
                      <span className="truncate font-medium">{t('pages.home.coreProblem')}</span>
                    </button>
                  )}

                  {/* Selected Server Status Tile (matches Sweden 1 / Отключено card) */}
                  <button
                    type="button"
                    onClick={() => setShowServerModal(true)}
                    className="w-full rounded-[14px] bg-[#262626]/75 hover:bg-[#262626]/90 backdrop-blur-md border border-white/10 px-3.5 py-2.5 flex items-center gap-3 text-left transition-colors cursor-pointer shadow-lg"
                  >
                    <span className="size-8 rounded-full bg-[#171717] border border-white/10 flex items-center justify-center shrink-0 overflow-hidden">
                      {parsedServer?.flag ? (
                        <span className="flag-emoji text-[19px] leading-none">
                          {parsedServer.flag}
                        </span>
                      ) : (
                        <Globe className="size-4 text-muted-foreground" />
                      )}
                    </span>

                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-[13.5px] font-medium text-foreground truncate">
                          {parsedServer?.title || firstGroup?.name || t('sider.proxyGroup')}
                        </span>
                        {activeProxyDelay > 0 && (
                          <span
                            className={cn(
                              'text-[11px] font-medium tabular-nums shrink-0',
                              activeProxyDelay < 100
                                ? 'text-[#4ADE80]'
                                : activeProxyDelay < 300
                                  ? 'text-[#FACC15]'
                                  : 'text-[#F87171]'
                            )}
                          >
                            {activeProxyDelay}ms
                          </span>
                        )}
                      </div>

                      <div className="flex items-center justify-between gap-2 mt-0.5 text-xs text-muted-foreground">
                        <span className="inline-flex items-center gap-1.5 truncate">
                          {isConnected && (
                            <span
                              className={cn(
                                'size-1.5 rounded-full shrink-0',
                                connectionMode === 'tun' ? 'bg-[#60A5FA]' : 'bg-[#4ADE80]'
                              )}
                            />
                          )}
                          <span>{statusText}</span>
                          {showConnectedTimer && (
                            <span className="tabular-nums text-foreground/80">
                              • <ConnectedTimer active={isConnected} />
                            </span>
                          )}
                        </span>

                        {isConnected && (
                          <span className="text-[11px] tabular-nums text-muted-foreground shrink-0">
                            ↓{calcTraffic(trafficInfo.down)}/s
                          </span>
                        )}
                      </div>
                    </div>

                    <ChevronsUpDown className="size-3.5 text-muted-foreground shrink-0" />
                  </button>

                  {/* 3-Segment Off / Proxy / TUN Control */}
                  <OffProxyTunSelector
                    mode={connectionMode}
                    loading={loading}
                    disableProxy={writeSysProxy && sysProxyMode === 'manual' && sysProxyDisabled}
                    onChange={handleModeChange}
                  />
                </div>
              </div>
            </div>
          </>
        )}

        {showEditModal && editingItem && (
          <EditInfoModal
            item={editingItem}
            isCurrent={false}
            updateProfileItem={async (item: ProfileItem) => {
              await addProfileItem(item)
              setShowEditModal(false)
              setEditingItem(null)
            }}
            onClose={() => {
              setShowEditModal(false)
              setEditingItem(null)
            }}
          />
        )}

        <QuickServerModal open={showServerModal} onClose={() => setShowServerModal(false)} />
      </div>
    </BasePage>
  )
}

export default Home
