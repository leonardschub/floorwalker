import type {
 AgentTokens,
 ProjectTokens,
 SessionTokens,
 StatisticsRange,
 TokenFigures,
 TokenStatistics,
} from './client'

/* The Tokens view's figures for browser development: the fixture tree's sessions and
   agents (fixture-client.ts) with invented tokens for Today, scaled up for 7 and 30
   days, plus an Unattributed row. Bars start at local midnight, so the chart's hours
   and days line up as they do on the desktop. */

type Kinds = Omit<TokenFigures, 'total' | 'share'>

const scale: Record<StatisticsRange, number> = { today: 1, '7d': 6, '30d': 23 }

function kinds(total: number): Kinds {
 // Mostly cache reads, as Claude Code's traffic is.
 const cacheRead = Math.round(total * 0.86)
 const cacheWrite = Math.round(total * 0.07)
 const output = Math.round(total * 0.02)
 return {
  cacheRead,
  cacheWrite,
  output,
  freshInput: total - cacheRead - cacheWrite - output,
 }
}

function sum(rows: TokenFigures[]): Kinds & { total: number } {
 const total = {
  total: 0,
  freshInput: 0,
  cacheRead: 0,
  cacheWrite: 0,
  output: 0,
 }
 for (const row of rows) {
  total.total += row.total
  total.freshInput += row.freshInput
  total.cacheRead += row.cacheRead
  total.cacheWrite += row.cacheWrite
  total.output += row.output
 }
 return total
}

type AgentSeed = [
 id: string,
 kind: 'root' | 'subagent',
 name: string,
 today: number,
]
type SessionSeed = [id: string, title: string | null, agents: AgentSeed[]]
type ProjectSeed = [id: string, name: string, sessions: SessionSeed[]]

const seeds: ProjectSeed[] = [
 [
  '-home-dev-Projects-AgentGamified',
  'AgentGamified',
  [
   [
    '6f1c2a4e-0b7d-4e2a-9c51-3d8e7fa10b2c',
    'Side panel wayfinder',
    [
     ['6f1c2a4e-0b7d-4e2a-9c51-3d8e7fa10b2c', 'root', '', 4_210_000],
     ['a3f81c09d2e4b5a67', 'subagent', 'general-purpose', 612_000],
     ['a9b04e7c1d35f2a88', 'subagent', 'Explore', 148_000],
    ],
   ],
   [
    'b3e9d0c7-5a21-4f6e-8d3b-7c2a1e9f4d60',
    'Fix reset token race',
    [['b3e9d0c7-5a21-4f6e-8d3b-7c2a1e9f4d60', 'root', '', 1_330_000]],
   ],
  ],
 ],
 [
  '-home-dev-Projects-Lantern',
  'Lantern',
  [
   [
    '0d4a7e1f-9c38-4b52-a6e0-5f1b8d2c7e93',
    'Translate chapter 12',
    [['0d4a7e1f-9c38-4b52-a6e0-5f1b8d2c7e93', 'root', '', 2_050_000]],
   ],
  ],
 ],
]

const unattributedToday = 41_000

/* Invented weights for the chart's bars: busier in the afternoon, quieter at
   weekends. */
const hourWeights: number[] = Array.from({ length: 24 }, (_, hour) =>
 hour < 8 ? 0 : hour < 12 ? 2 : hour < 19 ? 3 : 1,
)

function midnight(daysAgo: number): number {
 const day = new Date()
 day.setHours(0, 0, 0, 0)
 day.setDate(day.getDate() - daysAgo)
 return day.getTime()
}

function bars(range: StatisticsRange, total: number) {
 if (range === 'today') {
  const sumWeights = hourWeights.reduce((a, b) => a + b, 0)
  return hourWeights.map((weight, hour) => {
   const start = new Date(midnight(0))
   start.setHours(hour)
   return {
    start: start.getTime(),
    total: Math.round((total * weight) / sumWeights),
   }
  })
 }
 const days = range === '7d' ? 7 : 30
 const weights = Array.from(
  { length: days },
  (_, index) => 1 + ((index * 7) % 5) / 2,
 )
 const sumWeights = weights.reduce((a, b) => a + b, 0)
 return weights.map((weight, index) => ({
  start: midnight(days - 1 - index),
  total: Math.round((total * weight) / sumWeights),
 }))
}

export function fixtureTokens(range: StatisticsRange): TokenStatistics {
 const factor = scale[range]
 const projects = seeds.map(([id, name, sessions]) => {
  const sessionRows = sessions.map(([id, title, agents]) => {
   const agentRows: AgentTokens[] = agents.map(([id, kind, name, today]) => {
    const total = today * factor
    return { id, kind, name, total, share: 0, ...kinds(total) }
   })
   return { id, title, agents: agentRows, ...sum(agentRows), share: 0 }
  })
  return { id, name, sessions: sessionRows, ...sum(sessionRows), share: 0 }
 })
 const unattributedTotal = unattributedToday * factor
 const unattributed: TokenFigures = {
  total: unattributedTotal,
  share: 0,
  ...kinds(unattributedTotal),
 }
 const total = sum(projects).total + unattributedTotal
 const share = <T extends TokenFigures>(row: T): T => ({
  ...row,
  share: row.total / total,
 })
 const shared: ProjectTokens[] = projects
  .map((project) => ({
   ...share(project),
   sessions: project.sessions
    .map((session): SessionTokens => ({
     ...share(session),
     agents: session.agents.map(share),
    }))
    .sort((a, b) => b.total - a.total),
  }))
  .sort((a, b) => b.total - a.total)
 return {
  total,
  bucket: range === 'today' ? 'hour' : 'day',
  buckets: bars(range, total),
  projects: shared,
  unattributed: share(unattributed),
 }
}
