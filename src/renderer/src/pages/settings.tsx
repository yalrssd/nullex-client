import React, { useState } from 'react'
import { Button } from '@renderer/components/ui/button'
import BasePage from '@renderer/components/base/base-page'
import GeneralConfig from '@renderer/components/settings/general-config'
import AdvancedSettings from '@renderer/components/settings/advanced-settings'
import Actions from '@renderer/components/settings/actions'
import ShortcutConfig from '@renderer/components/settings/shortcut-config'
import AppearanceConfig from '@renderer/components/settings/appearance-confis'
import LanguageConfig from '@renderer/components/settings/language-config'
import ProxySwitches from '@renderer/components/settings/proxy-switches'
import { useTranslation } from 'react-i18next'
import {
  Github,
  Sliders,
  Palette,
  Cpu,
  Keyboard,
  Wrench,
  Layers,
  Sparkles
} from 'lucide-react'

type SettingsTab = 'general' | 'appearance' | 'network' | 'shortcuts' | 'system' | 'all'

const Settings: React.FC = () => {
  const { t, i18n } = useTranslation()
  const [activeTab, setActiveTab] = useState<SettingsTab>('general')
  const [showHiddenSettings, setShowHiddenSettings] = useState(false)
  const isRu = i18n.language?.toLowerCase().startsWith('ru')

  const tabDefs: { id: SettingsTab; label: string; icon: React.ReactNode; badge?: string }[] = [
    {
      id: 'general',
      label: isRu ? 'Основные' : 'General',
      icon: <Sliders className="w-4 h-4" />
    },
    {
      id: 'appearance',
      label: isRu ? 'Внешний вид' : 'Appearance',
      icon: <Palette className="w-4 h-4" />
    },
    {
      id: 'network',
      label: isRu ? 'Сеть и ядро' : 'Network & Core',
      icon: <Cpu className="w-4 h-4" />
    },
    {
      id: 'shortcuts',
      label: isRu ? 'Горячие клавиши' : 'Shortcuts',
      icon: <Keyboard className="w-4 h-4" />
    },
    {
      id: 'system',
      label: isRu ? 'Система' : 'System',
      icon: <Wrench className="w-4 h-4" />
    },
    {
      id: 'all',
      label: isRu ? 'Все настройки' : 'All Settings',
      icon: <Layers className="w-4 h-4" />
    }
  ]

  const tabMeta: Record<
    SettingsTab,
    { title: string; desc: string; tag: string }
  > = {
    general: {
      title: isRu ? 'Основные параметры' : 'General Configuration',
      desc: isRu
        ? 'Управление режимами маршрутизации (TUN / Системный прокси), автозапуском и языком.'
        : 'Manage traffic tunneling modes (TUN / System Proxy), startup behavior, and language.',
      tag: 'Core & Startup'
    },
    appearance: {
      title: isRu ? 'Персонализация интерфейса' : 'Appearance & Theme',
      desc: isRu
        ? 'Цветовые темы оформления, кастомные CSS-стили, плавающий виджет и сетка узлов.'
        : 'UI theme presets, custom CSS overrides, floating widget, and card layout styles.',
      tag: 'Styling'
    },
    network: {
      title: isRu ? 'Сетевой стек и ядро Mihomo' : 'Network Stack & Engine',
      desc: isRu
        ? 'Конфигурация локальных портов, движка Mihomo, URL и потоков для замера задержки.'
        : 'Inbound port bindings, Mihomo engine parameters, latency benchmark targets, and DNS.',
      tag: 'Engine'
    },
    shortcuts: {
      title: isRu ? 'Глобальные горячие клавиши' : 'Keyboard Shortcuts',
      desc: isRu
        ? 'Быстрый вызов окна приложения, переключение режимов проксирования и правил.'
        : 'Global hotkeys for instant window activation, proxy toggle, and rule switching.',
      tag: 'Hotkeys'
    },
    system: {
      title: isRu ? 'Система и обслуживание' : 'System & Maintenance',
      desc: isRu
        ? 'Проверка обновлений, перезапуск ядра, очистка кэша и управление конфигурацией.'
        : 'Release updater, core lifecycle controls, cache cleanup, and state management.',
      tag: 'Maintenance'
    },
    all: {
      title: isRu ? 'Все параметры Nullex' : 'All Nullex Settings',
      desc: isRu
        ? 'Полный перечень всех конфигурационных секций клиента в одном месте.'
        : 'Unified view of all client options, parameters, and system controls.',
      tag: 'Full View'
    }
  }

  const currentMeta = tabMeta[activeTab]

  return (
    <BasePage
      title={t('pages.settings.title')}
      header={
        <div className="flex items-center gap-2">
          <Button
            size="icon-sm"
            variant="ghost"
            className="app-nodrag hover:bg-white/10 transition-colors"
            title={t('pages.settings.githubRepo')}
            onClick={() => {
              window.open('https://github.com/yalrssd/nullex-client')
            }}
          >
            <Github className="w-4 h-4 text-foreground/80 hover:text-foreground" />
          </Button>
        </div>
      }
    >
      <div className="px-2 pb-6 space-y-4">
        {/* Modern Segmented Navigation Tabs */}
        <div className="flex flex-wrap items-center gap-1.5 p-1.5 bg-muted/40 backdrop-blur-md rounded-2xl border border-border/50 shadow-xs">
          {tabDefs.map((tab) => {
            const isActive = activeTab === tab.id
            return (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-3 py-1.5 rounded-xl text-xs font-medium transition-all duration-150 cursor-pointer ${
                  isActive
                    ? 'bg-primary text-primary-foreground shadow-sm scale-[1.02]'
                    : 'text-muted-foreground hover:text-foreground hover:bg-accent/40'
                }`}
              >
                {tab.icon}
                <span>{tab.label}</span>
              </button>
            )
          })}
        </div>

        {/* Section Header Card */}
        <div className="relative overflow-hidden rounded-2xl border border-primary/20 bg-gradient-to-r from-primary/10 via-background/60 to-secondary/10 p-4 shadow-xs backdrop-blur-sm">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-base font-semibold text-foreground tracking-tight">
                  {currentMeta.title}
                </h3>
                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-primary/15 text-primary border border-primary/25">
                  <Sparkles className="w-2.5 h-2.5" />
                  {currentMeta.tag}
                </span>
              </div>
              <p className="text-xs text-muted-foreground mt-1 max-w-xl">
                {currentMeta.desc}
              </p>
            </div>
            <div className="text-[11px] font-mono text-muted-foreground/80 shrink-0 self-start sm:self-center bg-card/60 px-2.5 py-1 rounded-lg border border-border/40">
              v1.1.0 • Tauri 2
            </div>
          </div>
        </div>

        {/* Render Tab Contents */}
        {(activeTab === 'general' || activeTab === 'all') && (
          <div className="space-y-2">
            {activeTab === 'all' && (
              <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-2 pt-2">
                {isRu ? '1. Режимы работы и система' : '1. Core & Startup'}
              </div>
            )}
            <ProxySwitches />
            <GeneralConfig showHiddenSettings={showHiddenSettings} />
            <LanguageConfig />
          </div>
        )}

        {(activeTab === 'appearance' || activeTab === 'all') && (
          <div className="space-y-2">
            {activeTab === 'all' && (
              <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-2 pt-4">
                {isRu ? '2. Оформление и темы' : '2. Appearance & Styling'}
              </div>
            )}
            <AppearanceConfig showHiddenSettings={showHiddenSettings} />
          </div>
        )}

        {(activeTab === 'network' || activeTab === 'all') && (
          <div className="space-y-2">
            {activeTab === 'all' && (
              <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-2 pt-4">
                {isRu ? '3. Сеть и ядро Mihomo' : '3. Network Stack'}
              </div>
            )}
            <AdvancedSettings showHiddenSettings={showHiddenSettings} />
          </div>
        )}

        {(activeTab === 'shortcuts' || activeTab === 'all') && (
          <div className="space-y-2">
            {activeTab === 'all' && (
              <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-2 pt-4">
                {isRu ? '4. Горячие клавиши' : '4. Shortcuts'}
              </div>
            )}
            <ShortcutConfig />
          </div>
        )}

        {(activeTab === 'system' || activeTab === 'all') && (
          <div className="space-y-2">
            {activeTab === 'all' && (
              <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-2 pt-4">
                {isRu ? '5. Обслуживание и утилиты' : '5. Maintenance'}
              </div>
            )}
            <Actions
              showHiddenSettings={showHiddenSettings}
              onUnlockHiddenSettings={() => setShowHiddenSettings(true)}
            />
          </div>
        )}
      </div>
    </BasePage>
  )
}

export default Settings
