import React from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  Activity,
  Compass,
  Globe,
  Layers,
  Route,
  ScrollText,
  Settings,
  X
} from 'lucide-react'
import { cn } from '@renderer/lib/utils'
import { platform } from '@renderer/utils/init'
import WindowControls from '@renderer/components/window-controls'
import UpdaterButton from '@renderer/components/updater/updater-button'
import { useControledMihomoConfig } from '@renderer/hooks/use-controled-mihomo-config'
import { useGroups } from '@renderer/hooks/use-groups'
import { useAppConfig } from '@renderer/hooks/use-app-config'
import { mihomoCloseAllConnections, patchMihomoConfig } from '@renderer/utils/ipc'
import { Tooltip, TooltipContent, TooltipTrigger } from '@renderer/components/ui/tooltip'
import nullexIcon from '@renderer/assets/nullex-appicon.svg'

interface AppSidebarProps {
  latest?: {
    version: string
    changelog: string
  }
}

const AppSidebar: React.FC<AppSidebarProps> = ({ latest }) => {
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()
  const isMac = platform === 'darwin'

  const { controledMihomoConfig, patchControledMihomoConfig } = useControledMihomoConfig()
  const { mutate: mutateGroups } = useGroups()
  const { appConfig } = useAppConfig()
  const { autoCloseConnection = true } = appConfig || {}
  const outboundMode = controledMihomoConfig?.mode

  const isSettingsOpen = location.pathname.startsWith('/settings')

  const navItems = [
    {
      path: '/home',
      label: t('sider.home'),
      icon: Compass,
      guide: 'sidebar-home-button'
    },
    {
      path: '/proxies',
      label: t('sider.proxyGroup'),
      icon: Globe
    },
    {
      path: '/profiles',
      label: t('sider.profileManagement'),
      icon: Layers
    },
    {
      path: '/connections',
      label: t('sider.connection'),
      icon: Activity
    },
    {
      path: '/rules',
      label: t('sider.rules'),
      icon: Route
    },
    {
      path: '/logs',
      label: t('sider.logs'),
      icon: ScrollText
    }
  ]

  const onChangeOutboundMode = async (mode: 'rule' | 'global'): Promise<void> => {
    if (outboundMode === mode) return
    await patchControledMihomoConfig({ mode })
    await patchMihomoConfig({ mode })
    if (autoCloseConnection) {
      await mihomoCloseAllConnections()
    }
    mutateGroups()
    window.electron.ipcRenderer.send('updateTrayMenu')
  }

  return (
    <header
      className={cn(
        'h-10 w-full shrink-0 bg-background border-b border-border flex items-center justify-between select-none app-drag z-50',
        isMac && 'pl-[70px]'
      )}
    >
      {/* Left: Brand + Minimal Navigation */}
      <div className="flex items-center h-full min-w-0">
        <button
          type="button"
          onClick={() => navigate('/home')}
          className="app-nodrag h-full px-3 flex items-center gap-2 hover:bg-accent/40 transition-colors cursor-pointer shrink-0"
        >
          <img src={nullexIcon} alt="Nullex" className="size-5 rounded-[5px] shrink-0" />
          <span className="text-[13px] font-medium tracking-tight text-foreground">Nullex</span>
        </button>

        <div className="h-4 w-px bg-border mx-1 hidden sm:block" />

        <nav
          data-guide="app-sidebar"
          className="app-nodrag flex items-center gap-0.5 px-1.5 min-w-0 overflow-x-auto no-scrollbar"
        >
          {navItems.map((item) => {
            const Icon = item.icon
            const active =
              item.path === '/home'
                ? location.pathname === '/' || location.pathname.startsWith('/home')
                : location.pathname.startsWith(item.path)

            return (
              <Tooltip key={item.path}>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    data-guide={item.guide}
                    onClick={() => navigate(item.path)}
                    className={cn(
                      'h-7 px-2.5 rounded-md inline-flex items-center gap-1.5 text-xs transition-colors cursor-pointer shrink-0',
                      active
                        ? 'bg-accent text-foreground font-medium'
                        : 'text-muted-foreground hover:text-foreground hover:bg-accent/50'
                    )}
                  >
                    <Icon className="size-3.5 shrink-0" />
                    <span className="hidden md:inline truncate">{item.label}</span>
                  </button>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="md:hidden">
                  {item.label}
                </TooltipContent>
              </Tooltip>
            )
          })}
        </nav>
      </div>

      {/* Right: Mode Toggle + Update + Settings Gear + Window Controls */}
      <div className="app-nodrag flex items-center h-full shrink-0">
        {outboundMode && (
          <div className="hidden lg:inline-flex items-center rounded-md bg-secondary/80 border border-border p-0.5 mr-1.5">
            <button
              type="button"
              onClick={() => onChangeOutboundMode('rule')}
              className={cn(
                'h-5.5 px-2 rounded-[5px] text-[11px] font-medium transition-colors cursor-pointer',
                outboundMode === 'rule'
                  ? 'bg-background text-foreground shadow-xs'
                  : 'text-muted-foreground hover:text-foreground'
              )}
            >
              {t('sider.rules')}
            </button>
            <button
              type="button"
              onClick={() => onChangeOutboundMode('global')}
              className={cn(
                'h-5.5 px-2 rounded-[5px] text-[11px] font-medium transition-colors cursor-pointer',
                outboundMode === 'global'
                  ? 'bg-background text-foreground shadow-xs'
                  : 'text-muted-foreground hover:text-foreground'
              )}
            >
              {t('common.global')}
            </button>
          </div>
        )}

        {latest?.version && (
          <div className="mr-1 flex items-center">
            <UpdaterButton iconOnly latest={latest} />
          </div>
        )}

        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => navigate(isSettingsOpen ? '/home' : '/settings')}
              className={cn(
                'w-[46px] h-10 inline-flex items-center justify-center transition-colors cursor-pointer',
                isSettingsOpen
                  ? 'bg-accent text-foreground'
                  : 'text-muted-foreground hover:text-foreground hover:bg-accent'
              )}
            >
              {isSettingsOpen ? <X className="size-3.5" /> : <Settings className="size-3.5" />}
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{t('common.settings')}</TooltipContent>
        </Tooltip>

        <WindowControls />
      </div>
    </header>
  )
}

export default AppSidebar
