import type { Agent, AgentTreeSnapshot } from './client'

/* The panel shows only what is still running, like the world: live sessions, and in
   them the agents still at work. A finished subagent stays only while one it spawned
   is still running, so that one keeps its place in the tree. Projects left with no
   live session go. */
export function running(tree: AgentTreeSnapshot): AgentTreeSnapshot {
 const projects = tree.projects
  .map((project) => ({
   ...project,
   sessions: project.sessions
    .filter((session) => session.live)
    .map((session) => ({ ...session, root: prune(session.root) })),
  }))
  .filter((project) => project.sessions.length > 0)
 return { ...tree, projects }
}

function prune(agent: Agent): Agent {
 const children = agent.children
  .map(prune)
  .filter((child) => child.state !== 'finished' || child.children.length > 0)
 return { ...agent, children }
}
