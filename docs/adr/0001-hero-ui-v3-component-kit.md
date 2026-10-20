# HeroUI v3 as the component kit

The 2d-app-gui retheme of the account, settings, and legal screens needed a component base. The design survey ranked shadcn/ui first, but shadcn distributes component source into the repository through its CLI, its npm surface covers Message Scroller and Questionnaire only, and the design-lock interview ruled copying component source out. HeroUI v3 is adopted as a normal dependency (`@heroui/react`, `@heroui/styles`), which follows the interview's preference for a maintained package whose defaults are kept; it requires React 19 and Tailwind v4, both already in place.

## Considered Options

- **shadcn/ui registry**: rejected. The registry model writes component source into the repo, and under a no-copy rule the components these screens need (Button, Field, Input, Checkbox, Select) are unavailable.
- **Unstyled primitives (Base UI, Radix Primitives, React Aria)**: rejected. Every style would be authored by hand, discarding the maintained visual defaults the interview chose to keep.
- **HeroUI v3**: chosen. Themeable through CSS variables the repo overrides, with reduced-motion handling and class/attribute dark mode built in.

## Consequences

The repo's design tokens are the source of truth for colour, type, spacing, radii, and motion, and HeroUI's own token values give way to them. Upstream owns component internals, so component changes arrive through package updates rather than local edits.
