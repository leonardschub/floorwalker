import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import { en } from './en'

void i18n.use(initReactI18next).init({
 lng: 'en',
 fallbackLng: 'en',
 resources: {
  en: { translation: en },
 },
 interpolation: { escapeValue: false },
 keySeparator: false,
 react: { useSuspense: false },
})

export { i18n }
export { Trans, useTranslation } from 'react-i18next'
