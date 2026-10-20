/* How the rest of the page reaches the world client (client.ts): the one place that
   imports it. The world lives in the Tauri shell, so the client exists only in the
   desktop build. The check below is landing.ts's `desktopBuild`, written out
   because the bundler folds `import.meta.env.MODE` only where it is read: through the
   imported constant, the browser bundle would still ship the client as a chunk. */

export type WorldClient = typeof import('./client')

type Cleanup = () => void

/** The world client, or null in the browser build. */
export function loadWorldClient(): Promise<WorldClient> | null {
 return import.meta.env.MODE === 'desktop' ? import('./client') : null
}

/** Runs `use` with the world client once it has loaded, for an effect. The returned
    function ends the run: `live()` tells `use` whether it has ended yet (check it after
    every await), and a cleanup `use` resolves with runs when the run ends, or at once
    if it already has. Does nothing in the browser build. */
export function withWorldClient(
 use: (
  client: WorldClient,
  live: () => boolean,
 ) => void | Cleanup | Promise<void | Cleanup>,
): Cleanup {
 let live = true
 let cleanup: Cleanup | null = null
 void loadWorldClient()?.then(async (client) => {
  if (!live) return
  const done = await use(client, () => live)
  if (!done) return
  if (live) cleanup = done
  else done()
 })
 return () => {
  live = false
  cleanup?.()
 }
}
