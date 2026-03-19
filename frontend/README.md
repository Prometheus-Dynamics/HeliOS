# Helios Frontend

Svelte 5 web interface for configuring and monitoring Helios camera streams. Talks to the backend HTTP API for registration, control, and live viewing.

## Stack & Requirements
- SvelteKit + TypeScript
- Tailwind CSS v4 + Skeleton UI
- Bun runtime and package manager (required)
- Node.js 18+

## Dev Quickstart
```bash
bun install
bun run dev
```

Other commands:
```bash
bun run check
bun run lint
bun run build
bun run preview
```

The frontend uses Bun as the package manager. Do not add other lockfiles or package-manager-specific config back into this workspace.

Codegen runs automatically before `dev`, `build`, `check`, and `lint`. To run it manually:
```bash
bun run codegen
```

## Environment
- `PUBLIC_API_BASE`: override API base URL. Defaults to the browser origin. If UI is served on `:5800`, it assumes API on `:5801`.
- `PUBLIC_ENABLE_LIGHTING_PERIPHERAL`: `auto` (default) shows Lighting only when detected; `true` always shows; `false` hides.

## Architecture Highlights
- App shell: `src/routes/+layout.svelte` (navigation rail + main workspace).
- Theme tokens: `src/themes/helios.css`; global styles in `src/app.css`; fonts in `src/fonts.css`.
- Primary routes: Dashboard, Devices (camera detail), Pipelines, Peers, Media, Localization, Systems, Settings, Docs.
- Shared systems: Stream viewer + overlays, pipeline graph editor, metrics and notifications.

## Design Language Essentials
- Dark, instrument-grade UI with `data-theme="helios"` and OKLCH token scales.
- Typeface: Geologica.
- Prefer existing primitives: `PageHeader`, `Panel`, `UsageTile`, `SegmentedBar`.
- Use uppercase + tracking for micro labels; keep spacing compact; reserve primary accent for key actions.

## UI Unification Priorities (Condensed)
- Standard primitives: `Button/IconButton`, `FormField` inputs, `StatusBadge/Tag`, `SearchBar/FilterChip`, `Alert/EmptyState`, `ModalShell/ConfirmDialog`, `DataList/Table`.
- Normalize headers/panels/tabs to reduce one-off layouts.
- Consolidate log/console surfaces and stream preview components.
- Enforce theme tokens (avoid raw colors) and add linting to prevent regressions.

## Style Rules
- Use `<script lang="ts">` in Svelte components.
- Keep pages under `src/routes`.
- Prefer `Promise.all` for concurrent API calls.
- Keep files near 600–800 LOC; split features and behavior into focused modules instead of oversized pages.
- Avoid stubs/pointless proxy files; if a file only re-exports or renders a single component, inline it unless there's a clear reason.
- Keep UI elements sharp and tool-like; avoid overly rounded corners unless the component already uses them.
