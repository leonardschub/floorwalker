import { useEffect } from 'react'

import { useTranslation } from '@floorwalker/i18n'

/* One route, one title: 2.4.2 asks that a page's title describe its topic or purpose, and
   until this existed the document carried the product name on every route.

   A page calls this with its own string, so nothing has to register a route anywhere
   else, and a legal page can pass the title out of its document's front matter. The
   product name comes from the wordmark, so a rename reaches every title at once. */
export function useDocumentTitle(title: string): void {
 const { t } = useTranslation()
 const product = t('index.wordmark')

 useEffect(() => {
  document.title = title === product ? product : `${title} · ${product}`
 }, [title, product])
}
