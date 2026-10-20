import type { ReactNode } from 'react'

import { Label, Meter } from '@heroui/react'

import type { AgentsClient, MonthCard, MonthEntry } from './client'
import { StatisticsAnswer, StatisticsNotice } from './statistics-parts'
import { useFormats } from './use-formats'
import { useStatistics } from './use-statistics'

/* The Of the month tab (spec "Statistics", "Of the month"): the model of the month
   and the harness of the month by total tokens over the current local calendar
   month, across every harness opencodex proxies. Each card shows the winner with its
   total and share, the next two runners-up with theirs, and last month's winner when
   the ledger has one. No range switch: the period is the calendar month. */

function ShareBar({
 entry,
 label,
 winner,
}: {
 entry: MonthEntry
 label: ReactNode
 winner: boolean
}) {
 const { percent } = useFormats()
 return (
  <Meter
   value={Math.min(100, Math.max(0, entry.share * 100))}
   valueLabel={percent(entry.share, 0)}
   color="accent"
   size="sm"
   className="gap-0.5"
  >
   {label}
   <Meter.Output className="text-mono font-normal text-muted" />
   <Meter.Track className="bg-raised">
    {/* Runners-up in the muted token, which reads in both themes; the kit's
        neutral fill is near-white in dark mode. */}
    <Meter.Fill className={winner ? undefined : 'bg-muted'} />
   </Meter.Track>
  </Meter>
 )
}

function Card({ title, card }: { title: string; card: MonthCard }) {
 const { t, percent, tokens, month } = useFormats()
 const [winner, ...runnersUp] = card.leaders
 return (
  <section className="flex flex-col gap-2">
   <h2 className="px-0.5 text-xs font-medium tracking-wide text-muted uppercase">
    {title}
   </h2>
   <div className="flex flex-col gap-2 rounded-lg border border-separator p-3">
    {winner === undefined ? (
     <StatisticsNotice text={t('agents.month.empty')} />
    ) : (
     <>
      <div className="flex min-w-0 flex-col">
       <span className="truncate text-base font-semibold text-foreground">
        {winner.name}
       </span>
       <span className="text-xs text-muted">
        {t('agents.month.winner', {
         tokens: tokens(winner.total),
         share: percent(winner.share, 0),
         month: month(),
        })}
       </span>
      </div>
      <ShareBar
       entry={winner}
       winner
       label={<Label className="sr-only">{winner.name}</Label>}
      />
      {runnersUp.length > 0 && (
       <ul
        aria-label={t('agents.month.runners-up')}
        className="m-0 flex list-none flex-col gap-1.5 p-0"
       >
        {runnersUp.map((entry) => (
         <li key={entry.id}>
          <ShareBar
           entry={entry}
           winner={false}
           label={
            <Label className="min-w-0 truncate text-xs font-normal text-foreground">
             {entry.name}
            </Label>
           }
          />
         </li>
        ))}
       </ul>
      )}
     </>
    )}
    {card.lastMonth !== null && (
     <span className="text-xs text-faint">
      {t('agents.month.last-month', { name: card.lastMonth })}
     </span>
    )}
   </div>
  </section>
 )
}

export default function MonthView({ client }: { client: AgentsClient }) {
 const { t } = useFormats()
 const answer = useStatistics(client, 'month', () => client.ofTheMonth())

 return (
  <StatisticsAnswer answer={answer}>
   {(figures) => (
    <div className="flex flex-col gap-4">
     <Card title={t('agents.month.model')} card={figures.model} />
     <Card title={t('agents.month.harness')} card={figures.harness} />
    </div>
   )}
  </StatisticsAnswer>
 )
}
