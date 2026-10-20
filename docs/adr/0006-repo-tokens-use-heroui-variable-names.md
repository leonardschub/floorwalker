# Repo tokens are authored on HeroUI's variable names

ADR-0001 chose HeroUI v3 as the component kit and left the repo's tokens as the source of truth, which left two ways to make that true: declare repo-owned names and bridge them onto the kit's variables, or write the repo's values onto the kit's own names. The repo writes its values onto the kit's names — `--background`, `--foreground`, `--surface`, `--field-background`, `--overlay`, `--muted`, `--separator`, `--accent`, `--accent-hover`, `--danger`, `--success` — in a `:root, .dark, [data-theme="dark"]` block and a `.light, [data-theme="light"]` block, and names the tokens the kit has no counterpart for (`raised`, `faint`, `separator-strong`, `accent-ink`, `danger-ink`, `focus`) in the same style. A bridge would double every name and leave two vocabularies to keep in step, while one vocabulary means every kit component themes from the repo's values without per-component work.

## Considered Options

- **Repo-owned names with a bridge onto the kit's variables**: rejected. Two names per colour, one more mapping to maintain, and no benefit while the kit is in use.
- **Repo-owned names with the kit's components restyled**: rejected by ticket 03, which keeps upstream component structure, behaviour and variants.

## Consequences

The repo's colour vocabulary is partly the kit's, so replacing the kit later means renaming tokens. The exact variable set is read off the installed `@heroui/styles` rather than from documentation examples.
