/* The agents client: the side panel's only way to its data (spec "The data contract").
   The panel takes a client as a prop and never imports Tauri, so the same component
   runs on fixtures in browser development and on the Rust agent registry in the
   desktop build. The shapes below are the contract with Rust: metadata only, and no
   field exists for message text, tool input or tool output.

   The statistics views (Of the month, Tokens, opencodex) have one request/response
   method each, taking the range (and, for opencodex, the surface filter) and
   answering view-ready figures or { error: 'opencodex-unavailable' }. While one is
   open it asks again when the client reports that opencodex's ledger grew. */

/** A root agent is busy, waiting (blocked on the user) or idle while live, and
    finished after; a subagent is running or finished. */
export type AgentState = 'busy' | 'waiting' | 'idle' | 'running' | 'finished'

/** A live root agent the user is holding up, as the registry decided it: amber
    *waiting on you* (idle, or blocked on a dialog), red *asking you something* (a
    question, a permission prompt or another request). */
export type Flag = {
 kind: 'waiting-on-you' | 'asking-you-something'
 /** In milliseconds since the epoch: from the last activity while waiting on you,
     from when the registry first saw the question while asking. */
 since: number
}

export type Agent = {
 /** The session id for a root agent, the agent id for a subagent. */
 id: string
 kind: 'root' | 'subagent'
 /** A subagent's agentType; the panel names a root agent itself. */
 name: string
 /** The model's display name, or null before the agent has answered. */
 model: string | null
 /** The model's id (`message.model` without opencodex's `ocx-claude-native--`), or
   null before the agent has answered. */
 modelId: string | null
 state: AgentState
 /** The name of the tool the agent is running now, never its input. */
 currentTool: string | null
 tokens: number
 /** Only ever on a live root agent that is idle or waiting. */
 flag: Flag | null
 children: Agent[]
}

export type Session = {
 id: string
 /** The session title, or null when the session has none yet. */
 title: string | null
 live: boolean
 /** Whether the user can sit in: a live background session with a process. */
 attachable: boolean
 /** The short id `claude attach` opens it by, only while attachable. */
 shortId: string | null
 /** The start (the transcript's first activity), in milliseconds since the epoch. */
 started: number
 /** The last activity, in milliseconds since the epoch. */
 lastActivity: number
 root: Agent
}

export type Project = {
 /** The projects/<project> directory name. */
 id: string
 /** The last path segment of the session's working directory. */
 name: string
 sessions: Session[]
}

export type AgentTreeSnapshot = {
 /** 'loading' while the registry's first build runs. */
 status: 'loading' | 'ready'
 projects: Project[]
}

export type GoToDesk = { sessionId: string; agentId: string }

/** A right-click on an agent's row (sit in): a root agent is sat in, from
    anywhere; a subagent has nothing to sit in. */
export type SitIn = GoToDesk & { kind: Agent['kind'] }

/** Why New session started nothing: an untrusted folder, no desk
    within the shell's deadline (`seconds` from the press), no Claude Code, no folder
    to start in, Claude Code's own message, or (`unknown`) a call that failed on the
    way (a denied command, a lost connection). */
export type NewSessionFailure =
 | { error: 'not-trusted' | 'claude-not-found' | 'no-folder' | 'unknown' }
 | { error: 'timed-out'; seconds: number }
 | { error: 'failed'; message: string }

/** A session New session started, once the agent tree lists it. */
export type StartedSession = { shortId: string; sessionId: string }

/** Starts a new background session in a project and waits for the agent tree to list
    it: resolves with it, or rejects with a {@link NewSessionFailure}. The shell keeps
    the deadline. */
export type StartSession = (projectId: string) => Promise<StartedSession>

export type Unsubscribe = () => void

/** The statistics ranges, in local time: from local midnight, or the last 7 or 30
    local calendar days including today. */
export type StatisticsRange = 'today' | '7d' | '30d'

/** Which harness's requests the opencodex view counts. "Surface" is opencodex's
    name for the field that says which harness sent a request; the code keeps it,
    while everything the user reads says harness. */
export type SurfaceFilter =
 'all' | 'claude-code' | 'codex' | 'claude-desktop' | 'grok-build'

export type OpencodexUnavailable = { error: 'opencodex-unavailable' }

export type LimitWindow = {
 kind: 'five-hour' | 'short' | 'weekly' | 'monthly' | 'custom'
 /** A custom window's own label (data), null for the others. */
 label: string | null
 /** How much of the window is used, 0 to 100. */
 percent: number
 /** When the window resets, in milliseconds since the epoch. */
 resetsAt: number | null
}

export type UsageLimit = {
 /** Labelled by its provider's id when opencodex knows it, otherwise numbered. */
 account: { provider: string } | { account: number }
 windows: LimitWindow[]
}

export type ModelFigures = {
 /** The served model's id. */
 model: string
 /** Its display name, or the id when it has none. */
 name: string
 provider: string
 requests: number
 /** 0 to 1. */
 errorRate: number
 medianFirstTokenMs: number | null
 medianTokensPerSecond: number | null
 /** Input read neither from nor into the cache, as in the Tokens view: fresh
     input, cache read, cache write and output sum to the total. */
 freshInput: number
 output: number
 /** Cache read. */
 cached: number
 cacheWrite: number
}

/** A harness the opencodex view's filter offers; its name is data, from Rust. */
export type Harness = { id: Exclude<SurfaceFilter, 'all'>; name: string }

export type OpencodexStatistics = {
 limits: UsageLimit[]
 summary: { requests: number; errors: number; failovers: number }
 /** Most requests first. */
 models: ModelFigures[]
 /** The harnesses the filter offers. */
 harnesses: Harness[]
}

/** A total and what it is made of, with its share of the range. The four kinds sum
    to the total; fresh input is input read neither from nor into the cache. */
export type TokenFigures = {
 total: number
 /** Of the range's total, 0 to 1. */
 share: number
 freshInput: number
 cacheRead: number
 cacheWrite: number
 output: number
}

export type AgentTokens = TokenFigures & {
 /** The session id for a root agent, the agent id for a subagent. */
 id: string
 kind: 'root' | 'subagent'
 /** A subagent's agentType; the panel names a root agent itself. */
 name: string
}

export type SessionTokens = TokenFigures & {
 id: string
 title: string | null
 /** The root agent first, then its subagents in the order they started. */
 agents: AgentTokens[]
}

export type ProjectTokens = TokenFigures & {
 id: string
 name: string
 /** Most tokens first. */
 sessions: SessionTokens[]
}

/** The Tokens view: Claude Code's tokens as opencodex counted them, attributed to
    agents by requestId. */
export type TokenStatistics = {
 total: number
 /** One bar per local hour (Today) or per local day. */
 bucket: 'hour' | 'day'
 /** Oldest first; start in milliseconds since the epoch. */
 buckets: { start: number; total: number }[]
 /** Most tokens first. */
 projects: ProjectTokens[]
 /** The rows that match no transcript; null when there are none. */
 unattributed: TokenFigures | null
}
/** A model (by version) or a harness, and its total tokens this local calendar
    month. */
export type MonthEntry = {
 /** The served model's id, or the harness's surface. */
 id: string
 /** The display name ("Opus 5.5", "Claude Code"), or the id when it has none. */
 name: string
 total: number
 /** Its share of the month's total, 0 to 1. */
 share: number
}

export type MonthCard = {
 /** The winner, then up to two runners-up; empty when nothing was used this
     month. */
 leaders: MonthEntry[]
 /** Last month's winner's name, null when the ledger has no last month. */
 lastMonth: string | null
}

/** The Of the month view's figures, across every harness opencodex proxies. */
export type OfTheMonth = { model: MonthCard; harness: MonthCard }

export interface AgentsClient {
 /** The agent tree as it is now. */
 snapshot(): Promise<AgentTreeSnapshot>
 /** Calls `handler` with the whole tree each time it changes, the ready tree after
     a loading one included. Resolves with the function that stops listening. */
 onSnapshot(handler: (tree: AgentTreeSnapshot) => void): Promise<Unsubscribe>
 /** The opencodex view's figures. */
 opencodexStatistics(
  range: StatisticsRange,
  surface: SurfaceFilter,
 ): Promise<OpencodexStatistics | OpencodexUnavailable>
 /** The Tokens view's figures. */
 tokenStatistics(
  range: StatisticsRange,
 ): Promise<TokenStatistics | OpencodexUnavailable>
 /** The Of the month view's figures, for the current local calendar month. */
 ofTheMonth(): Promise<OfTheMonth | OpencodexUnavailable>
 /** Calls `handler` each time opencodex's ledger grows. Resolves with the function
     that stops listening. */
 onLedgerGrew(handler: () => void): Promise<Unsubscribe>
}

/** The agents client this build uses: the Rust registry's in the desktop build, the
    fixtures in browser development, none in a production browser build. The checks
    are written out at each import because the bundler folds `import.meta.env` only
    where it is read, so the production browser bundle ships neither client. */
export function loadAgentsClient(): Promise<AgentsClient> | null {
 if (import.meta.env.MODE === 'desktop')
  return import('./tauri-client').then((module) => module.tauriClient)
 if (import.meta.env.DEV)
  return import('./fixture-client').then((module) => module.fixtureClient)
 return null
}
