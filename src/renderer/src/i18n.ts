import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import dayjs from 'dayjs'
import relativeTime from 'dayjs/plugin/relativeTime'
import localizedFormat from 'dayjs/plugin/localizedFormat'
import 'dayjs/locale/zh-cn'
import 'dayjs/locale/ru'
import 'dayjs/locale/en'

// Import translation files
import zhCN from './locales/zh-CN'
import enUS from './locales/en-US'
import ruRU from './locales/ru-RU'

const resources = {
  'zh-CN': { translation: zhCN },
  'en-US': { translation: enUS },
  'ru-RU': { translation: ruRU }
}

// Get saved language from localStorage or default to English (en-US)
const getSavedLanguage = (): string => {
  const saved = localStorage.getItem('language')
  if (saved && ['en-US', 'ru-RU'].includes(saved)) {
    return saved
  }

  return 'en-US'
}

const savedLanguage = getSavedLanguage()

// Configure dayjs globally
dayjs.extend(relativeTime)
dayjs.extend(localizedFormat)
const dayjsLocaleMap: Record<string, string> = {
  'zh-CN': 'zh-cn',
  'en-US': 'en',
  'ru-RU': 'ru'
}
dayjs.locale(dayjsLocaleMap[savedLanguage] || 'en')

i18n
  .use(initReactI18next)
  .init({
    resources,
    lng: savedLanguage,
    fallbackLng: 'en-US',
    interpolation: {
      escapeValue: false
    },
    react: {
      useSuspense: false
    }
  })

// Sync language to main process (tray, macOS menu bar)
window.electron.ipcRenderer.invoke('setLanguage', savedLanguage)

export default i18n
