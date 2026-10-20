/* The live region is always in the DOM, so a message that arrives later lands in a
   region that already existed. It carries no text while empty, which is why the idle
   state is visually hidden rather than unmounted. */
const shown =
 'block rounded-md border border-separator bg-raised px-4 py-3 text-sm text-foreground'

/* The danger tone is the same region over the danger ink: a screen paints it when the
   message reports a failed outcome rather than a field the person can correct. */
const dangerShown =
 'block rounded-md border border-danger/40 bg-danger-ink px-4 py-3 text-sm text-foreground'

function Notice({
 id,
 tone = 'default',
 children,
}: {
 id?: string
 tone?: 'default' | 'danger'
 children: string | null
}) {
 return (
  <output
   className={children ? (tone === 'danger' ? dangerShown : shown) : 'sr-only'}
   id={id}
  >
   {children}
  </output>
 )
}

export default Notice
