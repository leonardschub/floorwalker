import { useSyncExternalStore } from 'react'

import { loadWorldClient } from './world-client'

/* The world options on Settings (spec: "World settings"): device-local in this page's
   localStorage, like the theme preference, and sent whole to the shell as the world
   contract's settings when the world starts and whenever one changes. The shell's
   controller keeps the latest for a world launched later; the world never reads
   storage. Nothing here reaches the Worker. Desktop build only: the browser build has
   no world, and Settings shows no World section there. */

export const WORLD_SETTINGS_STORAGE_KEY = 'ag-world-settings'

export type WorldSettings = {
 /** How much smaller each level of subagent's robot is. The world floors the result
     at ×0.4; that floor is not a setting. */
 subagentScale: number
 textures: boolean
 clutter: boolean
 /** Off, punishments play without their squad, bolt, flash or blast, on the same
     timings and with the same end states. */
 punishmentEffects: boolean
 /** Walking speed in metres per second; Shift runs at 15/7 of it (ticket 28). */
 walkSpeed: number
 /** How far the mouse turns the view, as a multiple of 0.0025 radians per pixel. */
 lookSensitivity: number
}

/** The sliders' ranges and steps. The shell clamps to the same ranges
    (desktop/src-tauri/src/world/controls.rs). */
export const subagentScaleRange = { min: 0.5, max: 1, step: 0.05 } as const
export const walkSpeedRange = { min: 1, max: 4, step: 0.1 } as const
export const lookSensitivityRange = { min: 0.5, max: 3, step: 0.1 } as const

export const defaultWorldSettings: WorldSettings = {
 subagentScale: 0.75,
 textures: true,
 clutter: true,
 punishmentEffects: true,
 walkSpeed: 2.1,
 lookSensitivity: 1.5,
}

/** The two controls Reset puts back (ticket 28). */
export const defaultControls = {
 walkSpeed: defaultWorldSettings.walkSpeed,
 lookSensitivity: defaultWorldSettings.lookSensitivity,
} as const

const listeners = new Set<() => void>()

/* The options this page holds: read from storage once, then kept here, so a refused
   write still holds the choice for the session. */
let current: WorldSettings | null = null

/* A value on a slider's grid, or null. */
function onGrid(
 range: { min: number; max: number; step: number },
 value: unknown,
): number | null {
 if (typeof value !== 'number' || !Number.isFinite(value)) return null
 const { min, max, step } = range
 const steps = Math.round((Math.min(max, Math.max(min, value)) - min) / step)
 return Number((min + steps * step).toFixed(2))
}

const onScale = (value: unknown) => onGrid(subagentScaleRange, value)
const onWalkSpeed = (value: unknown) => onGrid(walkSpeedRange, value)
const onLookSensitivity = (value: unknown) =>
 onGrid(lookSensitivityRange, value)

/* Each option that is missing or unrecognized falls back to its default, and so does
   all of it when storage is unreadable. */
function read(): WorldSettings {
 try {
  const stored = window.localStorage.getItem(WORLD_SETTINGS_STORAGE_KEY)
  if (!stored) return defaultWorldSettings
  const parsed = JSON.parse(stored) as Partial<
   Record<keyof WorldSettings, unknown>
  >
  return {
   subagentScale:
    onScale(parsed.subagentScale) ?? defaultWorldSettings.subagentScale,
   textures:
    typeof parsed.textures === 'boolean'
     ? parsed.textures
     : defaultWorldSettings.textures,
   clutter:
    typeof parsed.clutter === 'boolean'
     ? parsed.clutter
     : defaultWorldSettings.clutter,
   punishmentEffects:
    typeof parsed.punishmentEffects === 'boolean'
     ? parsed.punishmentEffects
     : defaultWorldSettings.punishmentEffects,
   walkSpeed: onWalkSpeed(parsed.walkSpeed) ?? defaultWorldSettings.walkSpeed,
   lookSensitivity:
    onLookSensitivity(parsed.lookSensitivity) ??
    defaultWorldSettings.lookSensitivity,
  }
 } catch {
  return defaultWorldSettings
 }
}

export function getWorldSettings(): WorldSettings {
 current ??= read()
 return current
}

/** Sends the options to the shell. Does nothing in the browser build. */
export function sendWorldSettings(): void {
 const settings = getWorldSettings()
 void loadWorldClient()
  ?.then((client) => client.reportWorldSettings(settings))
  .catch(() => {
   // No shell to hear it; the next change or world start sends them again.
  })
}

/** Changes one or more options: stored on this device, then applied at once. */
export function setWorldSettings(change: Partial<WorldSettings>): void {
 const next = { ...getWorldSettings(), ...change }
 next.subagentScale =
  onScale(next.subagentScale) ?? defaultWorldSettings.subagentScale
 next.walkSpeed = onWalkSpeed(next.walkSpeed) ?? defaultWorldSettings.walkSpeed
 next.lookSensitivity =
  onLookSensitivity(next.lookSensitivity) ??
  defaultWorldSettings.lookSensitivity
 current = next

 try {
  window.localStorage.setItem(WORLD_SETTINGS_STORAGE_KEY, JSON.stringify(next))
 } catch {
  // This device holds the choice for the session only.
 }

 for (const listener of listeners) listener()
 sendWorldSettings()
}

function subscribe(listener: () => void): () => void {
 listeners.add(listener)
 return () => {
  listeners.delete(listener)
 }
}

export function useWorldSettings(): WorldSettings {
 return useSyncExternalStore(
  subscribe,
  getWorldSettings,
  () => defaultWorldSettings,
 )
}
