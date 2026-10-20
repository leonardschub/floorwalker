import { useTranslation } from '@floorwalker/i18n'
import Markdown, { type Components, type UrlTransform } from 'react-markdown'
import { Link } from 'react-router'
import remarkGfm from 'remark-gfm'

import { useDocumentTitle } from '../components/document-title'
import { parseLegalDocument } from './legal-front-matter.mjs'
import { legalLinkProtocol } from './legal-protocols.mjs'

/*
 * The legal document page: a card carrying the document's own title, its
 * last-updated date and the rendered body.
 *
 * The documents are compiled into the bundle as strings and parsed in the
 * browser by react-markdown, and the body starts at H2 because the title is the
 * page's only H1. The paragraph under the title is the front matter's
 * `lastUpdated` line, so the page and the document cannot disagree about the
 * date, and a generated document says so in a band under it.
 *
 * No sanitiser is present and none is needed: react-markdown drops raw HTML
 * unless `rehype-raw` is switched on, which is the one change that would make
 * `rehype-sanitize` mandatory in the same commit.
 */

const remarkPlugins = [remarkGfm]

/*
 * The allowlist itself lives in legal-protocols.mjs, which validate-legal.mjs
 * also reads. Here the transform throws while the document renders, and the
 * `a` override refuses an anchor that reaches it without a URL.
 */
function assertAllowedProtocol(url: string): string {
 const prefix = legalLinkProtocol(url)
 if (prefix === null) return url

 throw new Error(
  'A legal document links through the "' +
   prefix +
   '" protocol, which is not allowed: ' +
   url,
 )
}

const urlTransform: UrlTransform = (url) => assertAllowedProtocol(url)

const components: Components = {
 a({ node: _node, children, href, ...props }) {
  if (typeof href !== 'string' || href === '') {
   throw new Error(
    'A legal document link carries no URL, so its protocol cannot be checked.',
   )
  }

  assertAllowedProtocol(href)

  return (
   <a
    className="focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
    href={href}
    {...props}
   >
    {children}
   </a>
  )
 },
 pre({ node: _node, children, ...props }) {
  return (
   // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- WCAG 2.1.1: a focusable scroll container is the only keyboard route to a line that overflows.
   <div className="overflow-x-auto" tabIndex={0}>
    <pre {...props}>{children}</pre>
   </div>
  )
 },
 th({ node: _node, ...props }) {
  return <th scope="col" {...props} />
 },
}

function Band({ children }: { children: string }) {
 return (
  <p className="mt-6 rounded-sm border border-separator-strong bg-raised px-4 py-3 text-sm text-muted">
   {children}
  </p>
 )
}

/* The way back, to Settings' About section, where every legal page is opened from.
   The licence page (pages/license.tsx) shares it. */
export const backLinkClass =
 'text-sm text-accent underline underline-offset-4 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus'

function LegalPage({ source }: { source: string }) {
 const { t } = useTranslation()
 const { data, body } = parseLegalDocument(source)

 useDocumentTitle(data.title)

 return (
  <main className="mx-auto flex w-full max-w-[70ch] flex-col gap-6.5 py-10 max-[520px]:px-4 max-[520px]:py-6">
   <Link className={backLinkClass} to="/settings">
    {t('legal.back')}
   </Link>
   <article
    className="rounded-lg border border-separator bg-surface p-8 shadow-card max-[520px]:p-5"
    lang={data.language}
   >
    <h1 className="text-2xl font-semibold tracking-tight">{data.title}</h1>
    <p className="mt-1.5 text-mono text-faint">
     {t('legal.last-updated')}: {data.lastUpdated}
    </p>
    {data.generated ? <Band>{t('legal.generated')}</Band> : null}
    <div className="prose mt-5.5">
     <Markdown
      components={components}
      remarkPlugins={remarkPlugins}
      urlTransform={urlTransform}
     >
      {body}
     </Markdown>
    </div>
   </article>
  </main>
 )
}

export default LegalPage
