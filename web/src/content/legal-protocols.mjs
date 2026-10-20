/*
 * The link protocols a legal document may use.
 *
 * react-markdown's own URL transform silently drops anything else, which turns a
 * mistyped link into a dead one with nothing in a build to show for it, so the
 * page refuses a link outside this list while the document renders, and
 * web/scripts/validate-legal.mjs applies the same rule before a build runs. This
 * module is the one definition both read, so the page and the build cannot
 * disagree about what is allowed.
 */

export const LEGAL_LINK_PROTOCOLS = new Set([
 'http',
 'https',
 'irc',
 'ircs',
 'mailto',
 'xmpp',
])

/*
 * The offending protocol, or null when the URL is allowed. A colon that follows
 * a /, ? or # belongs to a relative URL rather than a protocol.
 */
export function legalLinkProtocol(url) {
 const colon = url.indexOf(':')
 if (colon === -1) return null

 const prefix = url.slice(0, colon)
 if (/[/?#]/.test(prefix)) return null
 if (LEGAL_LINK_PROTOCOLS.has(prefix.toLowerCase())) return null

 return prefix
}
