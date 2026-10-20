import { Suspense } from 'react'
import { useRoutes } from 'react-router'

import { useTranslation } from '@floorwalker/i18n'

import routes from '~react-pages'

import { desktopBuild } from './landing'
import QuitPrompt from './world/quit-prompt'

/* The app: its pages, and in the desktop build the quit prompt, which listens for a
   close of the main window whatever page is open. main.tsx mounts it under the
   browser's router; a test mounts it under a memory router. */
function App() {
 const { t } = useTranslation()

 return (
  <div className="flex min-h-screen flex-col">
   {desktopBuild ? <QuitPrompt /> : null}
   <Suspense fallback={<p>{t('loading')}</p>}>{useRoutes(routes)}</Suspense>
  </div>
 )
}

export default App
