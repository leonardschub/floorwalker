import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { LuPlus } from 'react-icons/lu'

import { Button, Tooltip } from '@heroui/react'
import { useTranslation } from '@floorwalker/i18n'

import type {
 Agent,
 AgentState,
 AgentTreeSnapshot,
 Flag,
 GoToDesk,
 NewSessionFailure,
 Session,
 SitIn,
} from './client'
import { flagged } from './format'
import { running } from './running'
import { useFormats } from './use-formats'
import { useSelection, type Selection } from './use-selection'

/* The Agents tab: project headings, then each live session's title, then its agents:
   the root agent and, indented under whoever spawned them, its subagents. Only what is
   still running shows (see `running`); a finished subagent kept for a running one it
   spawned is dimmed. Each has a state dot, the current tool (or the state) on the
   right, and model · tokens beneath. Pressing an agent outlines it and asks to go to
   its desk.

   A flagged root agent's dot takes the flag's colour (red and pulsing while it asks
   you something, amber and steady while it waits on you) and its right-hand text
   reads the flag and how long it has stood, ticking live. Pulsing holds steady under
   reduced motion. Only rows show flags: no counts on headings or the tab.

   The root agent the player sits in on the world screen (the seated view) is
   highlighted and its right-hand text reads seated. Where the panel is beside the
   world (`onSitIn`), right-clicking a row sits in it instead of opening the webview's
   menu: every row asks to sit in its agent (only a background session's root agent
   can be; the world screen gives the others their hint), and an attachable root
   agent's row is outlined as where the player goes. From the keyboard, the
   context-menu key or Shift+F10 on a focused row does the same, as either opens a
   context menu elsewhere.

   Beside the world (`newSession`), each project heading has a + (New session) that
   starts a background session in the project and sits the player in it once it
   appears; a start that fails leaves a red line under the heading (use-new-session.ts). */

const dotTone: Record<AgentState, string> = {
 busy: 'bg-success animate-pulse',
 running: 'bg-success',
 /* The registry flags every waiting agent, so this shows only for a debug
    timeline's unflagged one: as idle. */
 waiting: 'bg-accent',
 idle: 'bg-accent',
 finished: 'bg-faint',
}

const flagTone: Record<Flag['kind'], string> = {
 'asking-you-something': 'bg-danger animate-pulse motion-reduce:animate-none',
 'waiting-on-you': 'bg-warning',
}

function AgentRow({
 agent,
 sessionId,
 selection,
 seated,
 attachable,
 onSitIn,
}: {
 agent: Agent
 sessionId: string
 selection: Selection
 seated: boolean
 /** Whether this is the root agent of a session the player can sit in. */
 attachable: boolean
 onSitIn?: (target: SitIn) => void
}) {
 const { t, tokens } = useFormats()
 /* When a menu key last sat in from this row: the webview may follow the key with a
    contextmenu event of its own, which then asks nothing more. */
 const keyedAt = useRef<number | null>(null)
 const selected = selection.isSelected(sessionId, agent.id)
 const name = agent.kind === 'root' ? t('agents.root-agent') : agent.name
 const state = t(`agents.state.${agent.state}`)
 // Tokens arrive with the opencodex attribution, after the tree; until an agent
 // has any, the figure is left out rather than shown as 0.
 const count = agent.tokens > 0 ? tokens(agent.tokens) : null
 const details =
  agent.model !== null && count !== null
   ? t('agents.model-tokens', { model: agent.model, tokens: count })
   : (agent.model ?? count ?? '')
 const sitIn = () => {
  const target = { sessionId, agentId: agent.id }
  if (attachable) selection.outline(target)
  onSitIn?.({ ...target, kind: agent.kind })
 }

 return (
  <button
   type="button"
   aria-pressed={selected}
   aria-current={seated ? 'true' : undefined}
   data-session-id={sessionId}
   data-agent-id={agent.id}
   data-seated={seated ? 'true' : undefined}
   onClick={() => selection.select({ sessionId, agentId: agent.id })}
   onContextMenu={
    onSitIn &&
    ((event) => {
     event.preventDefault()
     const keyed = keyedAt.current
     keyedAt.current = null
     if (keyed !== null && event.timeStamp - keyed < 1_000) return
     sitIn()
    })
   }
   onKeyDown={
    onSitIn &&
    ((event) => {
     if (!isMenuKey(event)) return
     event.preventDefault()
     keyedAt.current = event.timeStamp
     sitIn()
    })
   }
   className={`flex min-h-6 w-full flex-col gap-0.5 rounded-sm px-1.5 py-1 text-left hover:bg-raised focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus aria-pressed:bg-raised aria-pressed:outline-1 aria-pressed:outline-accent ${seated ? 'bg-raised outline-1 outline-accent' : ''} ${agent.state === 'finished' ? 'opacity-60' : ''}`}
  >
   <span className="flex w-full min-w-0 items-center gap-2">
    <span
     aria-hidden="true"
     className={`inline-block size-2 shrink-0 rounded-full ${agent.flag === null ? dotTone[agent.state] : flagTone[agent.flag.kind]}`}
    />
    <span className="min-w-0 truncate text-sm text-foreground">{name}</span>
    {seated ? (
     <span className="ml-auto shrink-0 text-xs text-accent">
      {t('agents.seated')}
     </span>
    ) : agent.flag !== null ? (
     <FlagText flag={agent.flag} />
    ) : agent.currentTool === null ? (
     <span className="ml-auto shrink-0 text-xs text-muted">{state}</span>
    ) : (
     <>
      <span className="sr-only">{state}</span>
      <span className="ml-auto shrink-0 truncate text-mono text-muted">
       {agent.currentTool}
      </span>
     </>
    )}
   </span>
   {details === '' ? null : (
    <span className="truncate pl-4 text-xs text-faint">{details}</span>
   )}
  </button>
 )
}

/* The clock a flag's timer reads from, ticking once a second while it shows. */
function useSecondClock(): number {
 const [now, setNow] = useState(() => Date.now())
 useEffect(() => {
  const timer = window.setInterval(() => setNow(Date.now()), 1_000)
  return () => window.clearInterval(timer)
 }, [])
 return now
}

/* The keys that open a context menu on the focused element: the context-menu key and
   Shift+F10. */
const isMenuKey = (event: KeyboardEvent) =>
 event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')

/* A flag and how long it has stood: "Asking you something · 0:41". */
function FlagText({ flag }: { flag: Flag }) {
 const { t } = useTranslation()
 const now = useSecondClock()
 const name = t(`agents.flag.${flag.kind}`)
 const stood = flagged(flag.since, now)
 const text =
  stood.unit === 'seconds'
   ? t('agents.flag-for-seconds', {
      flag: name,
      seconds: String(stood.count).padStart(2, '0'),
     })
   : t(`agents.flag-for-${stood.unit}`, { flag: name, count: stood.count })
 return (
  <span
   className={`ml-auto shrink-0 text-xs ${flag.kind === 'asking-you-something' ? 'text-danger' : 'text-muted'}`}
  >
   {text}
  </span>
 )
}

/* An agent's row, then the agents it spawned, indented beneath it. */
function AgentBranch({
 agent,
 sessionId,
 selection,
 seated,
 attachable,
 onSitIn,
}: {
 agent: Agent
 sessionId: string
 selection: Selection
 seated: boolean
 attachable: boolean
 onSitIn?: (target: SitIn) => void
}) {
 return (
  <>
   <AgentRow
    agent={agent}
    sessionId={sessionId}
    selection={selection}
    seated={seated && agent.kind === 'root'}
    attachable={attachable && agent.kind === 'root'}
    onSitIn={onSitIn}
   />
   {agent.children.length === 0 ? null : (
    <ul className="m-0 ml-2.5 flex list-none flex-col gap-0.5 border-l border-separator p-0 pl-1.5">
     {agent.children.map((child) => (
      <li key={child.id} className="flex flex-col gap-0.5">
       <AgentBranch
        agent={child}
        sessionId={sessionId}
        selection={selection}
        seated={false}
        attachable={false}
        onSitIn={onSitIn}
       />
      </li>
     ))}
    </ul>
   )}
  </>
 )
}

function SessionBlock({
 session,
 selection,
 seated,
 onSitIn,
}: {
 session: Session
 selection: Selection
 seated: boolean
 onSitIn?: (target: SitIn) => void
}) {
 const { t } = useTranslation()

 return (
  <li className="flex flex-col gap-0.5">
   <div className="flex min-w-0 items-baseline gap-2 px-1.5 text-xs text-muted">
    <span className="min-w-0 truncate">
     {session.title ?? t('agents.untitled-session')}
    </span>
    <span className="ml-auto shrink-0 text-success">{t('agents.live')}</span>
   </div>
   <AgentBranch
    agent={session.root}
    sessionId={session.id}
    selection={selection}
    seated={seated}
    attachable={session.attachable}
    onSitIn={onSitIn}
   />
  </li>
 )
}

/* What New session offers the headings: see use-new-session.ts. */
export type NewSession = {
 starting: (projectId: string) => boolean
 failure: (projectId: string) => NewSessionFailure | null
 start: ((projectId: string) => void) | undefined
}

/* Why New session started nothing, as a whole sentence. */
function failureSentence(
 t: ReturnType<typeof useTranslation>['t'],
 failure: NewSessionFailure,
): string {
 switch (failure.error) {
  case 'timed-out':
   return t('agents.new-session-failed.timed-out', {
    seconds: failure.seconds,
   })
  case 'failed':
   return t('agents.new-session-failed.failed', { message: failure.message })
  default:
   return t(`agents.new-session-failed.${failure.error}`)
 }
}

/* A project's heading, with its + and, after a start that failed, why. */
function ProjectHeading({
 projectId,
 name,
 newSession,
}: {
 projectId: string
 name: string
 newSession?: NewSession
}) {
 const { t } = useTranslation()
 const start = newSession?.start
 const failure = newSession?.failure(projectId) ?? null
 const heading = (
  <h2 className="min-w-0 truncate text-xs font-medium tracking-wide text-muted uppercase">
   {name}
  </h2>
 )
 if (!start) return <div className="px-1.5">{heading}</div>
 return (
  <div className="flex flex-col gap-1 px-1.5">
   <div className="flex min-h-6 items-center gap-2">
    {heading}
    <Tooltip delay={300} closeDelay={0}>
     <Button
      isIconOnly
      size="sm"
      variant="outline"
      aria-label={t('agents.new-session')}
      data-new-session={projectId}
      isPending={newSession.starting(projectId)}
      onPress={() => start(projectId)}
      className="ml-auto size-6 min-w-6 shrink-0 text-sm"
     >
      <LuPlus aria-hidden="true" />
     </Button>
     <Tooltip.Content className="text-xs">
      {t('agents.new-session')}
     </Tooltip.Content>
    </Tooltip>
   </div>
   {/* Always there, so a failure is announced as it arrives. */}
   <p
    role="alert"
    data-new-session-failure={failure?.error}
    className="text-xs text-danger empty:sr-only"
   >
    {failure === null ? null : failureSentence(t, failure)}
   </p>
  </div>
 )
}

export default function AgentTreeView({
 tree,
 onGoToDesk,
 onSitIn,
 newSession,
 seatedSessionId = null,
}: {
 tree: AgentTreeSnapshot
 onGoToDesk: (target: GoToDesk) => void
 onSitIn?: (target: SitIn) => void
 /** The + on each project heading (beside the world only). */
 newSession?: NewSession
 seatedSessionId?: string | null
}) {
 const { t } = useTranslation()
 const selection = useSelection(onGoToDesk)

 if (tree.status === 'loading')
  return <p className="text-xs text-muted">{t('agents.loading')}</p>
 if (tree.projects.length === 0)
  return <p className="text-xs text-muted">{t('agents.empty')}</p>
 const shown = running(tree)
 if (shown.projects.length === 0)
  return <p className="text-xs text-muted">{t('agents.none-running')}</p>

 return (
  <div className="flex flex-col gap-4">
   {shown.projects.map((project) => (
    <section key={project.id} className="flex flex-col gap-2">
     <ProjectHeading
      projectId={project.id}
      name={project.name}
      newSession={newSession}
     />
     <ul className="m-0 flex list-none flex-col gap-2 p-0">
      {project.sessions.map((session) => (
       <SessionBlock
        key={session.id}
        session={session}
        selection={selection}
        seated={session.id === seatedSessionId}
        onSitIn={onSitIn}
       />
      ))}
     </ul>
    </section>
   ))}
  </div>
 )
}
