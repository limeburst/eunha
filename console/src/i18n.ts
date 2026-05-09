import { i18n } from '@lingui/core'
import { messages as enMessages } from './locales/en/messages.po'
import { messages as koMessages } from './locales/ko/messages.po'

export const locales = {
  en: 'English',
  ko: '한국어',
} as const

export type Locale = keyof typeof locales

const catalog: Record<Locale, typeof enMessages> = {
  en: enMessages,
  ko: koMessages,
}

export function activateLocale(locale: Locale) {
  i18n.loadAndActivate({ locale, messages: catalog[locale] })
}
