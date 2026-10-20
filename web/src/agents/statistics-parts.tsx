import type { ReactNode } from 'react'

import { useTranslation } from '@floorwalker/i18n'

import type { OpencodexUnavailable, StatisticsRange } from './client'
import type { Answer } from './use-statistics'

/* Pieces the statistics views (opencodex, Of the month, Tokens) share: the range
   switch, the one in-place error when opencodex is unavailable, the notices, and the
   wrapper that picks between them and the figures. */

const ranges: StatisticsRange[] = ['today', '7d', '30d']

const optionClass =
 'inline-flex min-h-6 items-center justify-center rounded-pill px-2 text-xs text-muted aria-pressed:bg-accent aria-pressed:text-accent-ink aria-pressed:shadow-control focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus'

/** Today · 7 days · 30 days, as a segmented switch. */
export function RangeSwitch({
 range,
 onChange,
}: {
 range: StatisticsRange
 onChange: (range: StatisticsRange) => void
}) {
 const { t } = useTranslation()
 return (
  <fieldset className="m-0 inline-flex min-w-0 shrink-0 items-center gap-0.5 rounded-pill border border-separator bg-field p-0.5">
   <legend className="sr-only">{t('agents.range')}</legend>
   {ranges.map((option) => (
    <button
     key={option}
     type="button"
     aria-pressed={range === option}
     className={optionClass}
     onClick={() => onChange(option)}
    >
     {t(`agents.range.${option}`)}
    </button>
   ))}
  </fieldset>
 )
}

/** The one in-place error a statistics view shows when opencodex is unavailable. */
export function OpencodexUnavailableNotice() {
 const { t } = useTranslation()
 return (
  <p
   role="alert"
   className="rounded-md border border-danger bg-raised p-3 text-xs text-danger"
  >
   {t('agents.opencodex-unavailable')}
  </p>
 )
}

export function StatisticsNotice({ text }: { text: string }) {
 return <p className="text-xs text-muted">{text}</p>
}

/** A statistics view's answer as the view shows it: a notice while it loads or if
    the request failed, the one in-place error when opencodex is unavailable, and
    otherwise whatever `children` makes of the figures. */
export function StatisticsAnswer<T extends object>({
 answer,
 children,
}: {
 answer: Answer<T | OpencodexUnavailable>
 children: (figures: T) => ReactNode
}) {
 const { t } = useTranslation()
 if (answer.failed)
  return <StatisticsNotice text={t('agents.statistics-failed')} />
 if (answer.value === null)
  return <StatisticsNotice text={t('agents.statistics-loading')} />
 if ('error' in answer.value) return <OpencodexUnavailableNotice />
 return children(answer.value)
}
