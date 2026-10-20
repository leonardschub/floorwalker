import { useState, type ReactNode } from 'react'

import { Label, Meter } from '@heroui/react'

import type {
 AgentsClient,
 Harness,
 LimitWindow,
 ModelFigures,
 StatisticsRange,
 SurfaceFilter,
 UsageLimit,
} from './client'
import {
 RangeSwitch,
 StatisticsAnswer,
 StatisticsNotice,
} from './statistics-parts'
import { useFormats } from './use-formats'
import { useStatistics } from './use-statistics'

/* The opencodex tab (spec "Statistics", "opencodex view"): what only opencodex knows,
   across every harness it proxies. A harness filter and the range on top, then each
   account's usage limits as bars with their reset times, the summary line, and one
   row per served model. No cost, no per-request list. The harnesses the filter
   offers, and their names, come with the figures from Rust. */

/** Past this a limit's bar turns to the danger tone. */
const NEARLY_USED = 80

function Section({ title, children }: { title: string; children: ReactNode }) {
 return (
  <section className="flex flex-col gap-2">
   <h2 className="px-0.5 text-xs font-medium tracking-wide text-muted uppercase">
    {title}
   </h2>
   {children}
  </section>
 )
}

function LimitBar({ window }: { window: LimitWindow }) {
 const { t, percent, dateTime } = useFormats()
 const name =
  window.kind === 'custom' && window.label !== null
   ? window.label
   : t(`agents.opencodex.window.${window.kind}`)
 const used = Math.min(100, Math.max(0, window.percent))
 return (
  <div className="flex flex-col gap-0.5">
   <Meter
    value={used}
    valueLabel={percent(used / 100, 0)}
    color={used >= NEARLY_USED ? 'danger' : 'accent'}
    size="sm"
    className="gap-0.5"
   >
    <Label className="min-w-0 truncate text-xs font-normal text-foreground">
     {name}
    </Label>
    <Meter.Output className="text-mono font-normal text-muted" />
    <Meter.Track className="bg-raised">
     <Meter.Fill />
    </Meter.Track>
   </Meter>
   {window.resetsAt !== null && (
    <span className="text-xs text-faint">
     {t('agents.opencodex.resets', { when: dateTime(window.resetsAt) })}
    </span>
   )}
  </div>
 )
}

function Limits({ limits }: { limits: UsageLimit[] }) {
 const { t } = useFormats()
 if (limits.length === 0)
  return <StatisticsNotice text={t('agents.opencodex.no-limits')} />
 return (
  <ul className="m-0 flex list-none flex-col gap-3 p-0">
   {limits.map((limit) => {
    const label =
     'provider' in limit.account
      ? limit.account.provider
      : t('agents.opencodex.account', { number: limit.account.account })
    return (
     <li key={label} className="flex flex-col gap-1.5">
      <span className="text-sm text-foreground">{label}</span>
      {limit.windows.map((window) => (
       <LimitBar key={`${window.kind}-${window.label}`} window={window} />
      ))}
     </li>
    )
   })}
  </ul>
 )
}

/* One figure of a model's row. Abbreviated labels (req, err, TTFT, tok/s) are shown
   as they are and read out in full. */
type Cell = { label: string; full?: string; value: string; warn?: boolean }

function ModelRow({ figures }: { figures: ModelFigures }) {
 const { t, percent, count, seconds, tokens } = useFormats()
 const none = t('agents.opencodex.not-reported')
 const cells: Cell[] = [
  {
   label: t('agents.opencodex.requests'),
   full: t('agents.opencodex.requests.full'),
   value: count(figures.requests),
  },
  {
   label: t('agents.opencodex.error-rate'),
   full: t('agents.opencodex.error-rate.full'),
   value: percent(figures.errorRate, 1),
   warn: figures.errorRate > 0.02,
  },
  {
   label: t('agents.opencodex.first-token'),
   full: t('agents.opencodex.first-token.full'),
   value:
    figures.medianFirstTokenMs === null
     ? none
     : seconds(figures.medianFirstTokenMs),
  },
  {
   label: t('agents.opencodex.tokens-per-second'),
   full: t('agents.opencodex.tokens-per-second.full'),
   value:
    figures.medianTokensPerSecond === null
     ? none
     : count(Math.round(figures.medianTokensPerSecond)),
  },
  {
   label: t('agents.opencodex.fresh-input'),
   value: tokens(figures.freshInput),
  },
  { label: t('agents.opencodex.output'), value: tokens(figures.output) },
  { label: t('agents.opencodex.cache-read'), value: tokens(figures.cached) },
  {
   label: t('agents.opencodex.cache-write'),
   value: tokens(figures.cacheWrite),
  },
 ]
 return (
  <li className="rounded-md border border-separator px-2 py-1.5">
   <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
    <span className="min-w-0 truncate text-sm text-foreground">
     {figures.name}
    </span>
    <span className="text-xs text-faint">{figures.provider}</span>
   </div>
   <dl className="m-0 mt-1 grid grid-cols-[repeat(auto-fill,minmax(6.5rem,1fr))] gap-x-2 gap-y-0.5">
    {cells.map(({ label, full, value, warn }) => (
     <div key={label} className="flex min-w-0 items-baseline gap-1">
      <dt className="shrink-0 text-xs text-faint">
       {full === undefined ? (
        label
       ) : (
        <>
         <span aria-hidden="true" title={full}>
          {label}
         </span>
         <span className="sr-only">{full}</span>
        </>
       )}
      </dt>
      <dd
       className={`m-0 min-w-0 truncate text-mono ${warn ? 'text-danger' : 'text-muted'}`}
      >
       {value}
      </dd>
     </div>
    ))}
   </dl>
  </li>
 )
}

/* The harness filter. Its options come with the figures; until the first answer
   only All is offered. */
function HarnessFilter({
 surface,
 harnesses,
 onChange,
}: {
 surface: SurfaceFilter
 harnesses: Harness[]
 onChange: (surface: SurfaceFilter) => void
}) {
 const { t } = useFormats()
 return (
  <label className="flex min-w-0 items-center gap-1.5 text-xs text-muted">
   <span className="sr-only">{t('agents.opencodex.harness')}</span>
   <select
    value={surface}
    onChange={(event) => onChange(event.target.value as SurfaceFilter)}
    className="min-h-6 min-w-0 rounded-sm border border-separator bg-field px-1.5 text-xs text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
   >
    <option value="all">{t('agents.opencodex.all-harnesses')}</option>
    {harnesses.map((harness) => (
     <option key={harness.id} value={harness.id}>
      {harness.name}
     </option>
    ))}
   </select>
  </label>
 )
}

export default function OpencodexView({ client }: { client: AgentsClient }) {
 const { t, count } = useFormats()
 const [range, setRange] = useState<StatisticsRange>('today')
 const [surface, setSurface] = useState<SurfaceFilter>('all')
 const [harnesses, setHarnesses] = useState<Harness[]>([])
 const answer = useStatistics(client, `${range}/${surface}`, () =>
  client.opencodexStatistics(range, surface),
 )
 const offered =
  answer.value !== null && !('error' in answer.value)
   ? answer.value.harnesses
   : null
 // The filter keeps the harnesses it was last offered while the next answer
 // loads, so the chosen one stays listed.
 if (offered !== null && offered !== harnesses) setHarnesses(offered)

 return (
  <div className="flex flex-col gap-4">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <HarnessFilter
     surface={surface}
     harnesses={offered ?? harnesses}
     onChange={setSurface}
    />
    <RangeSwitch range={range} onChange={setRange} />
   </div>
   <StatisticsAnswer answer={answer}>
    {({ limits, summary, models }) => (
     <>
      <Section title={t('agents.opencodex.limits')}>
       <Limits limits={limits} />
      </Section>
      <p className="rounded-sm bg-raised px-2 py-1.5 text-mono text-muted">
       {t('agents.opencodex.summary', {
        requests: count(summary.requests),
        errors: count(summary.errors),
        failovers: count(summary.failovers),
       })}
      </p>
      <Section title={t('agents.opencodex.models')}>
       {models.length === 0 ? (
        <StatisticsNotice text={t('agents.opencodex.no-requests')} />
       ) : (
        <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
         {models.map((figures) => (
          <ModelRow key={figures.model} figures={figures} />
         ))}
        </ul>
       )}
      </Section>
     </>
    )}
   </StatisticsAnswer>
  </div>
 )
}
