/* Small formatting for the panel's figures, kept apart from the markup. */

/** A token count in compact form: 86.4K, 4.2M. */
export function compactTokens(tokens: number, locale: string): string {
 return new Intl.NumberFormat(locale, {
  notation: 'compact',
  maximumFractionDigits: 1,
 }).format(tokens)
}

export type Since = { unit: 'minutes' | 'hours'; count: number }

/** How long a flag has stood: in seconds below a minute (ticking live, "0:41"), then
    as `since` counts. */
export function flagged(
 then: number,
 now: number,
): Since | { unit: 'seconds'; count: number } {
 const seconds = Math.max(0, Math.floor((now - then) / 1000))
 if (seconds < 60) return { unit: 'seconds', count: seconds }
 return since(then, now)
}

/** How long ago `then` was, in whole minutes below an hour (at least one) and in
    whole hours from there. */
export function since(then: number, now: number): Since {
 const minutes = Math.max(1, Math.floor((now - then) / 60_000))
 if (minutes < 60) return { unit: 'minutes', count: minutes }
 return { unit: 'hours', count: Math.floor(minutes / 60) }
}
