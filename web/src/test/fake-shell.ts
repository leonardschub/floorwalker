import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

import type { AgentTreeSnapshot } from '../agents/client'

/* A stand-in for the Tauri shell, at its IPC boundary: it answers the commands the
   pages send as an idle shell would, with one live session in the agent registry,
   and records every command, so a test can say what the app asked the shell for. A
   world that is asked to start reports itself running. */

export const liveSnapshot: AgentTreeSnapshot = {
 status: 'ready',
 projects: [
  {
   id: '-home-user-pong',
   name: 'pong',
   sessions: [
    {
     id: 'session-1',
     title: 'Make the paddles faster',
     live: true,
     attachable: true,
     shortId: 'ab12',
     started: 1_760_000_000_000,
     lastActivity: 1_760_000_060_000,
     root: {
      id: 'session-1',
      kind: 'root',
      name: 'pong',
      model: 'Opus',
      modelId: 'claude-opus',
      state: 'busy',
      currentTool: 'Edit',
      tokens: 1200,
      flag: null,
      children: [],
     },
    },
   ],
  },
 ],
}

export function fakeShell(): { asked: string[] } {
 const asked: string[] = []
 let world = 'none'

 mockIPC(
  (command) => {
   asked.push(command)
   switch (command) {
    case 'world_status':
     return world
    case 'world_start':
     world = 'running'
     return undefined
    case 'agents_snapshot':
     return liveSnapshot
    default:
     return undefined
   }
  },
  { shouldMockEvents: true },
 )

 return { asked }
}

export { clearMocks }
