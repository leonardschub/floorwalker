import { useEffect } from 'react'

import { useTranslation } from '@floorwalker/i18n'

import { withWorldClient } from './world-client'

/* The quit prompt (spec: "Quit prompt"). Mounted once at the app root in the desktop
   build. While a world exists the shell holds every close of the main window and asks
   this page to confirm; the page shows a native OS dialog with its own strings, so the
   shell holds no copy of them, and hands the answer back. Without a world the shell
   closes at once and never asks. A page that reloads while asking loses its prompt;
   the shell then asks the new page at the next close.

   The world client, which carries the dialog plugin, is loaded only in the desktop
   build (world-client.ts), so the browser bundle carries neither. */
function QuitPrompt() {
 const { t } = useTranslation()

 useEffect(() => {
  return withWorldClient((client) =>
   client.onQuitRequested(() => {
    client
     .confirmQuit({
      line: t('world.quit-line'),
      title: t('world.quit-title'),
      quit: t('world.quit'),
      cancel: t('world.quit-cancel'),
     })
     /* A dialog that cannot be shown must not trap the user in the app: they asked
         to close it, so the close goes ahead. */
     .catch(() => true)
     .then((confirmed) => client.answerQuit(confirmed))
     .catch(() => {})
   }),
  )
 }, [t])

 return null
}

export default QuitPrompt
