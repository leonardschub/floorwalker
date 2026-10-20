import { Channel, invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { confirm } from '@tauri-apps/plugin-dialog'
import { openUrl } from '@tauri-apps/plugin-opener'

import type { StartedSession } from '../agents/client'

/* The web side of the world contract (spec: "The world contract"). The world lives in
   the Tauri shell's Rust side (desktop/src-tauri/src/world), so this module exists only
   in the desktop build: it is reached only through world-client.ts, behind the
   build-time desktop check, and the browser bundle carries none of it. */

/** The world's state, as the shell's world controller reports it: *failed* after a
    start that did not reach a first frame, *stopped* after a running world crashed. */
export type WorldStatus = 'none' | 'starting' | 'running' | 'failed' | 'stopped'

/** Why a start did not reach a first frame: the shell's `StartFailure`. `world-cancelled`
    means a stop cancelled the start; every other token means the start failed. */
export type StartFailure =
 | 'world-cancelled'
 | 'world-no-window'
 | 'world-no-webview'
 | 'world-attach-failed'
 | 'world-thread-failed'
 | 'world-forced-failure'
 | 'world-panicked'
 | 'world-timeout'
 | 'world-assets-failed'
 | 'world-gone'

/** The world view's rectangle in physical pixels, relative to the webview's top-left. */
export type WorldRect = { x: number; y: number; w: number; h: number }

/** The current state. A screen that mounts reads this instead of assuming. */
export const worldStatus = () => invoke<WorldStatus>('world_status')

/** Starts the world, or joins a start in progress. Resolves once the first frame is
    drawn; resolves at once while a world is running. Rejects with a
    {@link StartFailure} token. */
export const startWorld = () => invoke<void>('world_start')

/** Whether the world screen shows the world: the world draws only while it does, and
    keeps simulating either way. */
export const reportWorldShown = (shown: boolean) =>
 invoke<void>('world_shown', { shown })

export const reportWorldRect = (rect: WorldRect) =>
 invoke<void>('world_rect', { rect })

/** Every status change the shell emits to this window. */
export const onWorldStatus = (
 handler: (status: WorldStatus) => void,
): Promise<UnlistenFn> =>
 listen<WorldStatus>('world-status', (event) => handler(event.payload))

/** Look deltas while the cursor is captured: mouse movement summed over one animation
    frame. */
export const sendLook = (dx: number, dy: number) =>
 invoke<void>('world_look', { dx, dy })

/** A button press while the cursor is captured (not the click that captured it): the
    world picks what the middle of the view aims at. The primary button works the CCTV
    console; the secondary sits in the robot within reach (sit in). Only a
    running world hears it; none is kept for a world launched later. */
export const sendClick = (button: 'primary' | 'secondary' = 'primary') =>
 invoke<void>('world_click', { button })

/** The seated view's seat (agent-screens spec: "The seated view"): the session sat in
    and its PC glass's rectangle in physical pixels relative to the webview's top-left,
    or null once the player has left (or the world has ended). */
export type WorldSeat = {
 sessionId: string
 rect: { x: number; y: number; w: number; h: number }
} | null

/** Every seat change: sitting in, the glass moving (a resize), leaving. */
export const onWorldSeat = (
 handler: (seat: WorldSeat) => void,
): Promise<UnlistenFn> =>
 listen<WorldSeat>('world-seat', (event) => handler(event.payload))

/** Why a sit-in can't be: a terminal-started session, or a subagent. */
export type WorldHint = 'terminal' | 'nothing'

/** Every sit-in that can't be, for the hint under the centre dot. */
export const onWorldHint = (
 handler: (hint: WorldHint) => void,
): Promise<UnlistenFn> =>
 listen<WorldHint>('world-hint', (event) => handler(event.payload))

/** Sits in a session's root agent from anywhere (the side panel's right-click): the
    camera goes to its desk and into the seat, from seat to seat while seated, keeping
    where the player first stood. A terminal-started session answers with the
    *terminal* hint instead. Only a world that exists hears it; none is kept for a
    world launched later. */
export const sitIn = (sessionId: string) =>
 invoke<void>('world_sit_in', { sessionId })

/** New session: starts `claude --bg` in the folder of the project's
    newest session and waits for the agent tree to list it. Resolves with the session
    once it does (the tree that lists it is published first, so the world has its
    desk); rejects with a `NewSessionFailure` (agents/client.ts), `timed-out` when the
    shell's deadline passed first. */
export const newSession = (projectId: string) =>
 invoke<StartedSession>('session_new', { projectId })

/** Leaves the seated view: back where the player stood. Nothing while not seated. */
export const leaveSeat = () => invoke<void>('world_leave')

/** The seated session's raw terminal output, as it comes, for the overlay. The shell
    ignores it unless `sessionId` is the seated session; it starts with a full redraw
    of the session's screen. The stream ends when the seat does. */
export const streamTerminal = (
 sessionId: string,
 onBytes: (bytes: Uint8Array) => void,
) => {
 const channel = new Channel<ArrayBuffer>()
 channel.onmessage = (data) => onBytes(new Uint8Array(data))
 return invoke<void>('terminal_stream', { sessionId, channel })
}

/** What the player typed in the seated view (keys, pastes and mouse reports, as the
    overlay's terminal encodes them), as raw bytes. The shell writes them to the seated
    session only, and ignores them unless `sessionId` is the seated session. */
export const terminalInput = (sessionId: string, bytes: Uint8Array) =>
 invoke<void>('terminal_input', bytes, { headers: { 'Session-Id': sessionId } })

/** Opens a link from the seated terminal in the default browser. The shell's opener
    takes http and https URLs only. */
export const openLink = (url: string) => openUrl(url)

/** The world options from Settings, sent whole (settings.ts). The shell keeps the
    latest for a world launched later. */
export const reportWorldSettings = (settings: {
 subagentScale: number
 textures: boolean
 clutter: boolean
 punishmentEffects: boolean
 walkSpeed: number
 lookSensitivity: number
}) => invoke<void>('world_settings', settings)

/** The world's fixed text in this page's language, the whole table (strings.ts): the
    world holds no strings of its own. The shell keeps the latest for a world launched
    later. */
export const reportWorldStrings = (strings: Record<string, string>) =>
 invoke<void>('world_strings', { strings })

/** Go to desk: the side panel's press on an agent. The player jumps beside its desk,
    turned to its robot; an agent with no desk changes nothing. A press while the world
    is starting applies once it runs. The cursor is not captured. */
export const goToDesk = (target: { sessionId: string; agentId: string }) =>
 invoke<void>('world_go_to_desk', target)

/** Whether the cursor is captured, on every Pointer Lock change and error: the world
    walks only while it is, and lets go of every held key when it is not. */
export const reportCaptured = (captured: boolean) =>
 invoke<void>('world_captured', { captured })

/** Stops the world completely: cancels a start in progress or ends a running world.
    Resolves once the world thread has ended and its surface is gone. Idempotent. */
export const stopWorld = () => invoke<void>('world_stop')

/** Asks this page to confirm quitting: the shell holds a close of the main window
    while a world exists and sends this once per prompt. Answer with `answerQuit`. */
export const onQuitRequested = (handler: () => void): Promise<UnlistenFn> =>
 listen('world-quit-requested', () => handler())

/** Shows the quit prompt as a native OS dialog; resolves with whether quitting was
    confirmed. The page hands it its strings, so the shell holds no copy of them. */
export const confirmQuit = (strings: {
 line: string
 title: string
 quit: string
 cancel: string
}) =>
 confirm(strings.line, {
  title: strings.title,
  kind: 'warning',
  okLabel: strings.quit,
  cancelLabel: strings.cancel,
 })

/** The page's answer to the quit prompt (the world contract's quit confirmation):
    confirmed stops the world in the controller's stop order and exits the app;
    cancelled changes nothing. */
export const answerQuit = (confirmed: boolean) =>
 invoke<void>('world_quit', { confirmed })
