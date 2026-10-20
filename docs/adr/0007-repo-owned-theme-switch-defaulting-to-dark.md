# Theme mode is resolved by a repo-owned switch that defaults to dark

HeroUI v3 drives its dark theme from CSS, applying it when `.dark` or `data-theme="dark"` sits on an ancestor, and ships a `useTheme` hook for Vite apps that persists its own storage key and sets both attributes after React mounts. Ticket 04 needs the mode settled before first paint and the stored preference to be ours, so the repo resolves the mode itself: `localStorage['ag-theme']` holds `light`, `dark` or `system`; anything else — absent, unrecognized, or storage unavailable — resolves to dark; `web/index.html` ships `class="dark" data-theme="dark"` statically and a blocking pre-paint snippet rewrites both when the stored preference says otherwise. A first-time visitor with a light operating system therefore sees the app dark until they choose otherwise, which is what dark-first means here.

## Considered Options

- **HeroUI's `useTheme` hook**: rejected. It runs after React mounts, so it cannot prevent the light flash, and its storage key would become part of the contract with the user's device without being ours to keep stable.
- **`next-themes`**: rejected. It solves this for Next.js, and the app is a Vite SPA with no prerender step.
- **`system` as the default for a first visit**: rejected. Dark-first is the app's stated posture, and `system` stays available as an explicit choice.

## Consequences

The storage key is a per-device contract: changing `ag-theme` silently discards every visitor's stored preference. The preference is not an account setting and does not travel between devices.
