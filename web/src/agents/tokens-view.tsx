import { useState, type ReactNode } from 'react'

import type {
 AgentsClient,
 AgentTokens,
 GoToDesk,
 ProjectTokens,
 SessionTokens,
 StatisticsRange,
 TokenFigures,
 TokenStatistics,
} from './client'
import {
 RangeSwitch,
 StatisticsAnswer,
 StatisticsNotice,
} from './statistics-parts'
import { useFormats } from './use-formats'
import { useSelection, type Selection } from './use-selection'
import { useStatistics } from './use-statistics'

/* The Tokens tab (spec "Statistics", "Tokens view"): where Claude Code's tokens went.
   The range's total and a small bar chart (per hour for Today, per day otherwise),
   then projects by total, each opening into its sessions and those into their agents.
   Every row shows its total and share of the range; opened, it shows fresh input,
   cache read, cache write and output. Pressing an agent outlines it and asks to go to
   its desk, as in the Agents tab. Rows that match no transcript sit on one
   Unattributed row. No cost. */

const toggleClass =
 'flex min-h-6 w-full min-w-0 items-center gap-1.5 rounded-sm px-1.5 py-1 text-left hover:bg-raised focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus'

/* The range's total per hour or per day: one hue, thin bars rising from a baseline,
   each named for screen readers and on hover. */
function Chart({ statistics }: { statistics: TokenStatistics }) {
 const { t, tokens, barStart } = useFormats()
 const { buckets, bucket } = statistics
 const highest = Math.max(1, ...buckets.map((bar) => bar.total))
 if (buckets.length === 0) return null
 const first = buckets[0]
 const last = buckets[buckets.length - 1]
 return (
  <figure className="m-0 flex flex-col gap-1">
   <figcaption className="sr-only">
    {t(`agents.tokens.chart.${bucket}`)}
   </figcaption>
   <ul className="m-0 flex h-16 list-none items-end gap-0.5 border-b border-separator p-0">
    {buckets.map((bar) => {
     const label = t('agents.tokens.bar', {
      when: barStart(bar.start, bucket),
      tokens: tokens(bar.total),
     })
     return (
      <li
       key={bar.start}
       className="group relative flex h-full min-w-0 flex-1 items-end"
      >
       <span className="sr-only">{label}</span>
       <span
        aria-hidden="true"
        className="block w-full rounded-t-xs bg-accent group-hover:opacity-80"
        style={{ height: `${(bar.total / highest) * 100}%` }}
       />
       <span
        aria-hidden="true"
        className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-1 hidden -translate-x-1/2 rounded-sm bg-raised px-1.5 py-0.5 text-mono whitespace-nowrap text-foreground shadow-overlay group-hover:block"
       >
        {label}
       </span>
      </li>
     )
    })}
   </ul>
   <div aria-hidden="true" className="flex justify-between text-xs text-faint">
    <span>{barStart(first.start, bucket)}</span>
    <span>{barStart(last.start, bucket)}</span>
   </div>
  </figure>
 )
}

/* An opened row's four kinds, which sum to its total. */
function Kinds({ figures }: { figures: TokenFigures }) {
 const { t, tokens } = useFormats()
 const cells: [string, number][] = [
  [t('agents.tokens.fresh-input'), figures.freshInput],
  [t('agents.tokens.cache-read'), figures.cacheRead],
  [t('agents.tokens.cache-write'), figures.cacheWrite],
  [t('agents.tokens.output'), figures.output],
 ]
 return (
  <dl className="m-0 grid grid-cols-[repeat(auto-fill,minmax(8rem,1fr))] gap-x-2 gap-y-0.5 px-1.5 pb-1">
   {cells.map(([label, value]) => (
    <div key={label} className="flex min-w-0 items-baseline gap-1">
     <dt className="shrink-0 text-xs text-faint">{label}</dt>
     <dd className="m-0 min-w-0 truncate text-mono text-muted">
      {tokens(value)}
     </dd>
    </div>
   ))}
  </dl>
 )
}

/* A row's name, then its total and share on the right. */
function Figures({
 name,
 figures,
}: {
 name: ReactNode
 figures: TokenFigures
}) {
 const { tokens, share } = useFormats()
 return (
  <>
   <span className="min-w-0 flex-1 truncate">{name}</span>
   <span className="shrink-0 text-mono text-foreground">
    {tokens(figures.total)}
   </span>
   <span className="w-10 shrink-0 text-right text-xs text-faint">
    {share(figures.share)}
   </span>
  </>
 )
}

function Chevron({ open }: { open: boolean }) {
 return (
  <span
   aria-hidden="true"
   className={`inline-block w-3 shrink-0 text-xs text-faint ${open ? 'rotate-90' : ''}`}
  >
   ›
  </span>
 )
}

/* A row that opens: its kinds, then whatever is inside it. */
function Expandable({
 name,
 figures,
 className,
 children,
}: {
 name: ReactNode
 figures: TokenFigures
 className: string
 children?: ReactNode
}) {
 const [open, setOpen] = useState(false)
 return (
  <li className="flex flex-col gap-0.5">
   <button
    type="button"
    aria-expanded={open}
    onClick={() => setOpen(!open)}
    className={`${toggleClass} ${className}`}
   >
    <Chevron open={open} />
    <Figures name={name} figures={figures} />
   </button>
   {open ? (
    <>
     <Kinds figures={figures} />
     {children}
    </>
   ) : null}
  </li>
 )
}

/* An agent: pressing it goes to its desk; the small button beside it opens its
   kinds. */
function AgentRow({
 agent,
 sessionId,
 selection,
}: {
 agent: AgentTokens
 sessionId: string
 selection: Selection
}) {
 const { t } = useFormats()
 const [open, setOpen] = useState(false)
 const selected = selection.isSelected(sessionId, agent.id)
 const name = agent.kind === 'root' ? t('agents.root-agent') : agent.name
 return (
  <li className="flex flex-col gap-0.5">
   <div className="flex min-w-0 items-center gap-0.5">
    <button
     type="button"
     aria-expanded={open}
     aria-label={t('agents.tokens.details', { name })}
     onClick={() => setOpen(!open)}
     className="inline-flex size-6 shrink-0 items-center justify-center rounded-sm hover:bg-raised focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
    >
     <Chevron open={open} />
    </button>
    <button
     type="button"
     aria-pressed={selected}
     onClick={() => selection.select({ sessionId, agentId: agent.id })}
     className={`${toggleClass} text-sm text-foreground aria-pressed:bg-raised aria-pressed:outline-1 aria-pressed:outline-accent`}
    >
     <Figures name={name} figures={agent} />
    </button>
   </div>
   {open ? <Kinds figures={agent} /> : null}
  </li>
 )
}

function SessionRow({
 session,
 selection,
}: {
 session: SessionTokens
 selection: Selection
}) {
 const { t } = useFormats()
 return (
  <Expandable
   name={session.title ?? t('agents.untitled-session')}
   figures={session}
   className="text-xs text-muted"
  >
   <ul className="m-0 ml-2.5 flex list-none flex-col gap-0.5 border-l border-separator p-0 pl-1.5">
    {session.agents.map((agent) => (
     <AgentRow
      key={agent.id}
      agent={agent}
      sessionId={session.id}
      selection={selection}
     />
    ))}
   </ul>
  </Expandable>
 )
}

function ProjectRow({
 project,
 selection,
}: {
 project: ProjectTokens
 selection: Selection
}) {
 return (
  <Expandable
   name={project.name}
   figures={project}
   className="text-sm font-medium text-foreground"
  >
   <ul className="m-0 ml-2.5 flex list-none flex-col gap-0.5 border-l border-separator p-0 pl-1.5">
    {project.sessions.map((session) => (
     <SessionRow key={session.id} session={session} selection={selection} />
    ))}
   </ul>
  </Expandable>
 )
}

export default function TokensView({
 client,
 onGoToDesk,
}: {
 client: AgentsClient
 onGoToDesk: (target: GoToDesk) => void
}) {
 const { t, tokens } = useFormats()
 const [range, setRange] = useState<StatisticsRange>('today')
 const selection = useSelection(onGoToDesk)
 const answer = useStatistics(client, range, () =>
  client.tokenStatistics(range),
 )

 return (
  <div className="flex flex-col gap-4">
   <div className="flex justify-end">
    <RangeSwitch range={range} onChange={setRange} />
   </div>
   <StatisticsAnswer answer={answer}>
    {(statistics) => (
     <>
      <p className="m-0 flex items-baseline gap-2">
       <span className="text-base text-foreground">
        {tokens(statistics.total)}
       </span>
       <span className="text-xs text-muted">{t('agents.tokens.total')}</span>
      </p>
      <Chart statistics={statistics} />
      {statistics.total === 0 ? (
       <StatisticsNotice text={t('agents.tokens.none')} />
      ) : (
       <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
        {statistics.projects.map((project) => (
         <ProjectRow key={project.id} project={project} selection={selection} />
        ))}
        {statistics.unattributed === null ? null : (
         <Expandable
          name={t('agents.tokens.unattributed')}
          figures={statistics.unattributed}
          className="text-sm text-muted"
         />
        )}
       </ul>
      )}
     </>
    )}
   </StatisticsAnswer>
  </div>
 )
}
