import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { I18nProvider } from '@lingui/react'
import { i18n } from '@lingui/core'
import App from './App'
import './index.css'
import { activateLocale, locales, type Locale } from './i18n'

function detectLocale(): Locale {
  const lang = navigator.language.split('-')[0].toLowerCase()
  return (lang in locales) ? lang as Locale : 'en'
}

const saved = localStorage.getItem('console_locale')
const locale: Locale = (saved && saved in locales) ? saved as Locale : detectLocale()
activateLocale(locale)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <I18nProvider i18n={i18n}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </I18nProvider>
  </StrictMode>,
)
