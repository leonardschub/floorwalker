import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import type {
 AgentsClient,
 AgentTreeSnapshot,
 OfTheMonth,
 OpencodexStatistics,
 OpencodexUnavailable,
 TokenStatistics,
} from './client'

/* The agents client of the desktop build: the agent registry in the Tauri shell's Rust
   side (desktop/src-tauri/src/agents.rs, the `agents` crate; ADR-0010). Reached only
   through loadAgentsClient's build-time desktop check, so the browser bundle carries
   none of it. The first snapshot starts the registry's build and reads 'loading'; the
   ready tree follows as an event, and so does every later change to it (the registry
   watches the files and sends at most about one a second). */

/** The listener registered last. A snapshot asked for after it waits until the shell
    has it: a quick first build could otherwise send its ready tree before anyone
    listens, leaving the page on 'loading'. */
let listening: Promise<unknown> = Promise.resolve()

export const tauriClient: AgentsClient = {
 snapshot: () =>
  listening.then(() => invoke<AgentTreeSnapshot>('agents_snapshot')),
 onSnapshot: (handler) => {
  const unlisten = listen<AgentTreeSnapshot>('agents-snapshot', (event) =>
   handler(event.payload),
  )
  listening = unlisten.catch(() => undefined)
  return unlisten
 },
 opencodexStatistics: (range, surface) =>
  invoke<OpencodexStatistics | OpencodexUnavailable>(
   'agents_opencodex_statistics',
   { range, surface },
  ),
 tokenStatistics: (range) =>
  invoke<TokenStatistics | OpencodexUnavailable>('agents_token_statistics', {
   range,
  }),
 ofTheMonth: () =>
  invoke<OfTheMonth | OpencodexUnavailable>('agents_of_the_month'),
 onLedgerGrew: (handler) => listen('opencodex-ledger-grew', () => handler()),
}
