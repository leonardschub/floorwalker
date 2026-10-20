import { useTranslation } from '@floorwalker/i18n'
import { Link } from 'react-router'

import licence from '../../../LICENSE?raw'

import { useDocumentTitle } from '../components/document-title'
import { backLinkClass } from '../content/legal-page'

/* The licence the app is published under, GNU AGPL-3.0, read inside the app (local-only
   spec, Legal: section 0's "a way to read the licence"). The text is the repository's
   own LICENSE, compiled into the bundle as it stands, so the page and the file cannot
   disagree. It is plain text, laid out by its own line breaks, so it is shown as
   preformatted text rather than parsed as Markdown. */
function License() {
 const { t } = useTranslation()
 useDocumentTitle(t('license.title'))

 return (
  <main className="mx-auto flex w-full max-w-[82ch] flex-col gap-6.5 py-10 max-[520px]:px-4 max-[520px]:py-6">
   <Link className={backLinkClass} to="/settings">
    {t('legal.back')}
   </Link>
   <article className="rounded-lg border border-separator bg-surface p-8 shadow-card max-[520px]:p-5">
    <h1 className="sr-only">{t('license.title')}</h1>
    {/* oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- WCAG 2.1.1: a focusable scroll container is the only keyboard route to a line that overflows. */}
    <div className="overflow-x-auto" tabIndex={0}>
     <pre className="text-mono leading-relaxed whitespace-pre">{licence}</pre>
    </div>
   </article>
  </main>
 )
}

export default License
