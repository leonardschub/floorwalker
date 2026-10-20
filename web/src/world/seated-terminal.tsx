import { useEffect, useRef } from 'react'

import '@xterm/xterm/css/xterm.css'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { WebglAddon } from '@xterm/addon-webgl'
import { Terminal } from '@xterm/xterm'

/* The world's own copy of JetBrains Mono (OFL-1.1), bundled with the page: the PCs'
   images are drawn in it too (terminals crate). */
import fontUrl from '../../../desktop/world-assets/third-party/jetbrains-mono/JetBrainsMono[wght].ttf?url'

import type { WorldSeat } from './client'

/* The seated view's terminal (agent-screens spec: "The seated view"): an xterm.js 6
   terminal of the attach's fixed 100 × 30, laid flush on the PC's glass with no border
   or padding, fed the seated session's raw output (`stream`), which starts with the
   full redraw the size retake brings. WebGL renders it, falling back to the DOM
   renderer if the context is lost; Unicode 11 widths; no scrollback (attached sessions
   are fullscreen and scroll themselves). The font size is the largest that fits the
   grid in the glass, and whatever the grid leaves of the glass is the glass's black.
   Loaded only in the desktop build, and only once the player sits in.

   Typing (spec: "Keys, paste and links"): the terminal takes focus as it opens and
   claims every key, Tab, Esc, F5, Ctrl+W, Ctrl+R and Ctrl+Q included; what it encodes
   (keys, pastes, mouse reports, its query replies) goes to `input`, in order, a send
   at a time. Ctrl+Shift+C copies the selection and Ctrl+Shift+V pastes (bracketed when
   the session asks for it), through the terminal's own copy and paste handling; plain
   Ctrl+C goes to the session. Claude Code turns on mouse tracking, so mouse events are
   its own; Shift+drag makes a plain selection. Ctrl+click on a link (found in the text,
   or an OSC 8 hyperlink) opens it with `openLink`, http and https only. */

const COLUMNS = 100
const ROWS = 30
const FAMILY = 'JetBrains Mono'

let font: Promise<unknown> | null = null
const loadFont = () => {
 if (!font) {
  const face = new FontFace(FAMILY, `url("${fontUrl}")`, {
   weight: '100 800',
  })
  document.fonts.add(face)
  font = face.load().catch(() => undefined)
 }
 return font
}

type Seat = NonNullable<WorldSeat>

/* The glass's black (the `glass` token), for the terminal's own background. */
const glassColour = () =>
 getComputedStyle(document.documentElement).getPropertyValue('--glass').trim()

/** Whether a link may be opened: http and https only. */
const webLink = (uri: string) => /^https?:\/\//i.test(uri)

/* Sends in order, one at a time: what arrives while a send is out goes next, joined
   (mouse motion reports come in bursts). */
function sender(send: (bytes: Uint8Array) => Promise<unknown>) {
 const encoder = new TextEncoder()
 let waiting: Uint8Array[] = []
 let out = false
 const next = () => {
  if (out || waiting.length === 0) return
  const bytes = new Uint8Array(waiting.reduce((n, part) => n + part.length, 0))
  let at = 0
  for (const part of waiting) {
   bytes.set(part, at)
   at += part.length
  }
  waiting = []
  out = true
  void send(bytes)
   .catch((error: unknown) => console.warn('[seat] typing lost', error))
   .finally(() => {
    out = false
    next()
   })
 }
 return {
  /** Text from the terminal, as UTF-8. */
  text: (data: string) => {
   waiting.push(encoder.encode(data))
   next()
  },
  /** A binary string from the terminal (old-style mouse reports): a byte a char. */
  binary: (data: string) => {
   waiting.push(Uint8Array.from(data, (c) => c.charCodeAt(0) & 0xff))
   next()
  },
 }
}

/* Ctrl+Shift+C and Ctrl+Shift+V, claimed before the terminal encodes them (it would
   send them as Ctrl+C and Ctrl+V). The terminal's hidden textarea has focus, so the
   copy and paste commands fire its own handlers: the selection is copied, and a paste
   is bracketed when the session asks for it. Returns whether the terminal may encode
   the key. */
function clipboardKeys(term: Terminal, event: KeyboardEvent): boolean {
 if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey)
  return true
 if (event.code !== 'KeyC' && event.code !== 'KeyV') return true
 event.preventDefault()
 if (event.type !== 'keydown') return false
 if (event.code === 'KeyC') {
  if (term.hasSelection() && !document.execCommand('copy'))
   void navigator.clipboard
    ?.writeText(term.getSelection())
    .catch((error: unknown) => console.warn('[seat] could not copy', error))
 } else if (!document.execCommand('paste')) {
  void navigator.clipboard
   ?.readText()
   .then((text) => term.paste(text))
   .catch((error: unknown) => console.warn('[seat] could not paste', error))
 }
 return false
}

export default function SeatedTerminal({
 seat,
 stream,
 input,
 openLink,
}: {
 seat: Seat
 stream: (sessionId: string, onBytes: (bytes: Uint8Array) => void) => void
 input: (sessionId: string, bytes: Uint8Array) => Promise<unknown>
 openLink: (url: string) => void
}) {
 const glass = useRef<HTMLDivElement>(null)
 const host = useRef<HTMLDivElement>(null)
 const terminal = useRef<Terminal | null>(null)
 const { sessionId } = seat
 const dpr = window.devicePixelRatio || 1
 const box = {
  left: seat.rect.x / dpr,
  top: seat.rect.y / dpr,
  width: seat.rect.w / dpr,
  height: seat.rect.h / dpr,
 }

 /* One terminal per seated session, fed its stream from the start. */
 useEffect(() => {
  let live = true
  let made: Terminal | null = null
  /* A Ctrl+clicked link, found in the text or an OSC 8 hyperlink. */
  const openWebLink = (event: MouseEvent, uri: string) => {
   if (event.ctrlKey && webLink(uri)) openLink(uri)
  }
  void loadFont()
   .then(() => {
    if (!live || !host.current) return
    made = new Terminal({
     cols: COLUMNS,
     rows: ROWS,
     scrollback: 0,
     allowProposedApi: true,
     fontFamily: `"${FAMILY}", monospace`,
     fontSize: 12,
     theme: { background: glassColour() },
     /* OSC 8 hyperlinks: opened like found links, never by xterm.js's own prompt. */
     linkHandler: { activate: openWebLink, allowNonHttpProtocols: false },
    })
    made.loadAddon(new Unicode11Addon())
    made.unicode.activeVersion = '11'
    made.loadAddon(new WebLinksAddon(openWebLink))
    made.open(host.current)
    try {
     const webgl = new WebglAddon()
     webgl.onContextLoss(() => {
      console.warn('[seat] the terminal lost its WebGL context: DOM renderer')
      webgl.dispose()
     })
     made.loadAddon(webgl)
    } catch (error) {
     console.warn('[seat] no WebGL for the terminal: DOM renderer', error)
    }
    terminal.current = made
    fit(made, glass.current)
    const term = made
    const typed = sender((bytes) =>
     live ? input(sessionId, bytes) : Promise.resolve(),
    )
    term.attachCustomKeyEventHandler((event) => clipboardKeys(term, event))
    term.onData(typed.text)
    term.onBinary(typed.binary)
    stream(sessionId, (bytes) => {
     if (live) term.write(bytes)
    })
    term.focus()
   })
   .catch((error: unknown) => console.warn('[seat] the terminal failed', error))
  return () => {
   live = false
   made?.dispose()
   terminal.current = null
  }
 }, [sessionId, stream, input, openLink])

 /* The glass moved or resized: fit the grid again. */
 useEffect(() => {
  if (terminal.current) fit(terminal.current, glass.current)
 }, [box.width, box.height])

 return (
  <div
   ref={glass}
   data-world-region="seated-terminal"
   data-session-id={sessionId}
   className="fixed flex items-center justify-center overflow-hidden bg-glass"
   style={box}
  >
   <div ref={host} />
  </div>
 )
}

/* The largest font size at which the grid fits in the glass: scaled to fit from a
   measure, then stepped down while it overflows (cells snap to device pixels, so the
   scaled size may be a little too big). */
function fit(term: Terminal, glass: HTMLElement | null) {
 const screen = term.element?.querySelector<HTMLElement>('.xterm-screen')
 if (!glass || !screen) return
 const room = glass.getBoundingClientRect()
 const grid = () => screen.getBoundingClientRect()
 const first = grid()
 if (first.width === 0 || first.height === 0) return
 const size = term.options.fontSize ?? 12
 const scale = Math.min(room.width / first.width, room.height / first.height)
 term.options.fontSize = Math.floor(size * scale * 4) / 4
 for (let step = 0; step < 24; step++) {
  const now = grid()
  if (now.width <= room.width && now.height <= room.height) return
  term.options.fontSize = (term.options.fontSize ?? size) - 0.25
 }
}
