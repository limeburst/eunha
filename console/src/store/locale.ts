import { create } from 'zustand'
import { activateLocale, locales, type Locale } from '../i18n'
import { setLocale as setLocaleApi } from '../api/endpoints'
import { useAuthStore } from './auth'

const STORAGE_KEY = 'console_locale'

interface LocaleState {
  locale: Locale
  setLocale: (locale: Locale) => void
}

function detectLocale(): Locale {
  const lang = navigator.language.split('-')[0].toLowerCase()
  return (lang in locales) ? lang as Locale : 'en'
}

function initialLocale(): Locale {
  const saved = localStorage.getItem(STORAGE_KEY)
  return (saved && saved in locales) ? saved as Locale : detectLocale()
}

const stored = initialLocale()

export const useLocaleStore = create<LocaleState>((set) => ({
  locale: stored,
  setLocale: (locale) => {
    activateLocale(locale)
    localStorage.setItem(STORAGE_KEY, locale)
    set({ locale })
    if (useAuthStore.getState().token) {
      setLocaleApi(locale).catch(() => {})
    }
  },
}))
