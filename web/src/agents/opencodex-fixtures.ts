import type {
 ModelFigures,
 OpencodexStatistics,
 StatisticsRange,
 SurfaceFilter,
} from './client'

/* The opencodex view's figures for browser development. Each harness has a few
   invented models for Today; 7 and 30 days scale them up, and All adds the harnesses
   together, so switching the range or the filter changes every figure as it does on
   the desktop. Reset times are relative to when the module loads. */

const loadedAt = Date.now()
const hour = 60 * 60 * 1000

type Harness = Exclude<SurfaceFilter, 'all'>

type Figures = Omit<ModelFigures, 'errorRate'> & { errors: number }

const today: Record<Harness, { failovers: number; models: Figures[] }> = {
 'claude-code': {
  failovers: 1,
  models: [
   {
    model: 'claude-opus-5-5',
    name: 'Opus 5.5',
    provider: 'anthropic-native',
    requests: 412,
    errors: 9,
    medianFirstTokenMs: 6480,
    medianTokensPerSecond: 110,
    freshInput: 1_240_000,
    output: 386_000,
    cached: 41_900_000,
    cacheWrite: 3_491_666,
   },
   {
    model: 'gpt-6.1-sol',
    name: 'GPT-6.1 Sol',
    provider: 'openai',
    requests: 96,
    errors: 3,
    medianFirstTokenMs: 6630,
    medianTokensPerSecond: 52,
    freshInput: 310_000,
    output: 88_000,
    cached: 7_400_000,
    cacheWrite: 616_666,
   },
  ],
 },
 codex: {
  failovers: 2,
  models: [
   {
    model: 'deepseek-flash',
    name: 'deepseek-flash',
    provider: 'deepseek',
    requests: 380,
    errors: 2,
    medianFirstTokenMs: 890,
    medianTokensPerSecond: 236,
    freshInput: 2_100_000,
    output: 240_000,
    cached: 18_000_000,
    cacheWrite: 1_500_000,
   },
   {
    model: 'gpt-6.1-sol',
    name: 'GPT-6.1 Sol',
    provider: 'openai',
    requests: 22,
    errors: 1,
    medianFirstTokenMs: 5900,
    medianTokensPerSecond: 61,
    freshInput: 92_000,
    output: 30_000,
    cached: 1_100_000,
    cacheWrite: 91_666,
   },
  ],
 },
 'claude-desktop': {
  failovers: 0,
  models: [
   {
    model: 'claude-sonnet-5-5',
    name: 'Sonnet 5.5',
    provider: 'anthropic',
    requests: 14,
    errors: 0,
    medianFirstTokenMs: 1200,
    medianTokensPerSecond: 148,
    freshInput: 64_000,
    output: 12_000,
    cached: 380_000,
    cacheWrite: 31_666,
   },
  ],
 },
 'grok-build': {
  failovers: 0,
  models: [
   {
    model: 'grok-5',
    name: 'grok-5',
    provider: 'xai',
    requests: 31,
    errors: 4,
    medianFirstTokenMs: null,
    medianTokensPerSecond: null,
    freshInput: 0,
    output: 0,
    cached: 0,
    cacheWrite: 0,
   },
  ],
 },
}

const scale: Record<StatisticsRange, number> = { today: 1, '7d': 6, '30d': 23 }

export function fixtureStatistics(
 range: StatisticsRange,
 surface: SurfaceFilter,
): OpencodexStatistics {
 const factor = scale[range]
 const harnesses: Harness[] =
  surface === 'all'
   ? ['claude-code', 'codex', 'claude-desktop', 'grok-build']
   : [surface]
 const byModel = new Map<string, Figures>()
 let failovers = 0
 for (const harness of harnesses) {
  failovers += today[harness].failovers * factor
  for (const figures of today[harness].models) {
   const seen = byModel.get(figures.model)
   byModel.set(
    figures.model,
    seen === undefined
     ? { ...figures }
     : {
        ...seen,
        requests: seen.requests + figures.requests,
        errors: seen.errors + figures.errors,
        freshInput: seen.freshInput + figures.freshInput,
        output: seen.output + figures.output,
        cached: seen.cached + figures.cached,
        cacheWrite: seen.cacheWrite + figures.cacheWrite,
       },
   )
  }
 }
 const models = [...byModel.values()]
  .map(({ errors, ...figures }) => ({
   ...figures,
   requests: figures.requests * factor,
   errorRate: errors / figures.requests,
   freshInput: figures.freshInput * factor,
   output: figures.output * factor,
   cached: figures.cached * factor,
   cacheWrite: figures.cacheWrite * factor,
  }))
  .sort((a, b) => b.requests - a.requests)
 const requests = models.reduce((sum, model) => sum + model.requests, 0)
 const errors = models.reduce(
  (sum, model) => sum + Math.round(model.errorRate * model.requests),
  0,
 )
 return {
  limits: [
   {
    account: { provider: 'codex' },
    windows: [
     {
      kind: 'five-hour',
      label: null,
      percent: 12,
      resetsAt: loadedAt + 2 * hour,
     },
     {
      kind: 'weekly',
      label: null,
      percent: 43,
      resetsAt: loadedAt + 76 * hour,
     },
    ],
   },
   {
    account: { provider: 'anthropic' },
    windows: [
     {
      kind: 'weekly',
      label: null,
      percent: 71,
      resetsAt: loadedAt + 100 * hour,
     },
     {
      kind: 'custom',
      label: 'Fable',
      percent: 18,
      resetsAt: loadedAt + 100 * hour,
     },
    ],
   },
   {
    account: { account: 1 },
    windows: [
     {
      kind: 'five-hour',
      label: null,
      percent: 96,
      resetsAt: loadedAt + hour / 3,
     },
    ],
   },
  ],
  summary: { requests, errors, failovers },
  models,
  // The names Rust sends (Surface::name); data, not translations.
  harnesses: [
   { id: 'claude-code', name: 'Claude Code' },
   { id: 'codex', name: 'Codex' },
   { id: 'claude-desktop', name: 'Claude Desktop' },
   { id: 'grok-build', name: 'Grok Build' },
  ],
 }
}
