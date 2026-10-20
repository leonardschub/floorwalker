import type { Agent, AgentsClient, AgentTreeSnapshot } from './client'
import { fixtureMonth } from './month-fixtures'
import { fixtureStatistics } from './opencodex-fixtures'
import { fixtureTokens } from './tokens-fixtures'

/* The agents client of browser development: a fixed tree, so the panel can be worked
   on with `pnpm dev` and no desktop shell. Times are relative to when the module
   loads, so the finished session always reads a few hours old. Model, project and
   session names are invented. */

const loadedAt = Date.now()
const hour = 60 * 60 * 1000

type Fields = Pick<
 Agent,
 'model' | 'modelId' | 'state' | 'currentTool' | 'tokens'
> &
 Partial<Pick<Agent, 'children' | 'flag'>>

function root(sessionId: string, fields: Fields): Agent {
 return {
  id: sessionId,
  kind: 'root',
  name: '',
  children: [],
  flag: null,
  ...fields,
 }
}

function subagent(id: string, agentType: string, fields: Fields): Agent {
 return {
  id,
  kind: 'subagent',
  name: agentType,
  children: [],
  flag: null,
  ...fields,
 }
}

const tree: AgentTreeSnapshot = {
 status: 'ready',
 projects: [
  {
   id: '-home-dev-Projects-AgentGamified',
   name: 'AgentGamified',
   sessions: [
    {
     id: '6f1c2a4e-0b7d-4e2a-9c51-3d8e7fa10b2c',
     title: 'Side panel wayfinder',
     live: true,
     attachable: true,
     shortId: '6f1c2a4e',
     started: loadedAt - 95 * 60 * 1000,
     lastActivity: loadedAt - 2 * 60 * 1000,
     root: root('6f1c2a4e-0b7d-4e2a-9c51-3d8e7fa10b2c', {
      model: 'Opus 5.5',
      modelId: 'claude-opus-5-5',
      state: 'busy',
      currentTool: 'Bash',
      tokens: 4_210_000,
      children: [
       subagent('a3f81c09d2e4b5a67', 'general-purpose', {
        model: 'Sonnet 5.5',
        modelId: 'claude-sonnet-5-5',
        state: 'running',
        currentTool: 'Agent',
        tokens: 612_000,
        children: [
         subagent('a9b04e7c1d35f2a88', 'Explore', {
          model: 'Haiku 5.5',
          modelId: 'claude-haiku-5-5',
          state: 'running',
          currentTool: 'Grep',
          tokens: 148_000,
         }),
        ],
       }),
       subagent('a51d6e2b7c90f4e13', 'ocx-gpt-6-luna', {
        model: 'GPT-6 Luna',
        modelId: 'gpt-6-luna',
        state: 'finished',
        currentTool: null,
        tokens: 0,
       }),
      ],
     }),
    },
    {
     id: 'b3e9d0c7-5a21-4f6e-8d3b-7c2a1e9f4d60',
     title: 'Fix reset token race',
     live: false,
     attachable: false,
     shortId: null,
     started: loadedAt - 4 * hour,
     lastActivity: loadedAt - 3 * hour,
     root: root('b3e9d0c7-5a21-4f6e-8d3b-7c2a1e9f4d60', {
      model: 'Sonnet 5.5',
      modelId: 'claude-sonnet-5-5',
      state: 'finished',
      currentTool: null,
      tokens: 1_330_000,
     }),
    },
    {
     id: '7a2d9e4b-1c86-4f03-b5a7-e9d3c1f60b48',
     title: 'Rename the settings keys',
     live: true,
     attachable: false,
     shortId: null,
     started: loadedAt - 30 * 60 * 1000,
     lastActivity: loadedAt - 40 * 1000,
     root: root('7a2d9e4b-1c86-4f03-b5a7-e9d3c1f60b48', {
      model: 'Opus 5.5',
      modelId: 'claude-opus-5-5',
      state: 'waiting',
      currentTool: 'Bash',
      tokens: 380_000,
      flag: { kind: 'asking-you-something', since: loadedAt - 35 * 1000 },
     }),
    },
   ],
  },
  {
   id: '-home-dev-Projects-Lantern',
   name: 'Lantern',
   sessions: [
    {
     id: '0d4a7e1f-9c38-4b52-a6e0-5f1b8d2c7e93',
     title: 'Translate chapter 12',
     live: true,
     attachable: false,
     shortId: null,
     started: loadedAt - 2 * hour,
     lastActivity: loadedAt - 11 * 60 * 1000,
     root: root('0d4a7e1f-9c38-4b52-a6e0-5f1b8d2c7e93', {
      model: 'DeepSeek V4',
      modelId: 'deepseek-v4',
      state: 'idle',
      currentTool: null,
      tokens: 2_050_000,
      flag: { kind: 'waiting-on-you', since: loadedAt - 11 * 60 * 1000 },
     }),
    },
    {
     id: 'e8c51b3a-2d6f-4a90-b7e4-1a3c9d5f0e27',
     title: null,
     live: false,
     attachable: false,
     shortId: null,
     started: loadedAt - 20 * hour,
     lastActivity: loadedAt - 19 * hour,
     root: root('e8c51b3a-2d6f-4a90-b7e4-1a3c9d5f0e27', {
      model: 'GPT-6.1 Sol',
      modelId: 'gpt-6.1-sol',
      state: 'finished',
      currentTool: null,
      tokens: 86_400,
     }),
    },
   ],
  },
 ],
}

export const fixtureClient: AgentsClient = {
 snapshot: () => Promise.resolve(tree),
 // The fixtures never change, so there is nothing to listen for.
 onSnapshot: () => Promise.resolve(() => {}),
 opencodexStatistics: (range, surface) =>
  Promise.resolve(fixtureStatistics(range, surface)),
 tokenStatistics: (range) => Promise.resolve(fixtureTokens(range)),
 ofTheMonth: () => Promise.resolve(fixtureMonth),
 // Nor does the fixture ledger grow.
 onLedgerGrew: () => Promise.resolve(() => {}),
}
