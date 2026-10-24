# Topics Index

> Module-level maps of this repository, one per side of the Tauri seam.

Last updated: 2026-09-29

## Contents

| File | Summary | Tags | Updated |
|------|---------|------|---------|
| [code-map-overview.md](code-map-overview.md) | The four Rust module groups, the frontend layers, where the seam sits, and the five files that hold most of the enforcement. | code-map, architecture | 2026-09-29 |
| [backend-module-map.md](backend-module-map.md) | `src-tauri` module by module: `agent/` decides, `services/` touches the platform, `commands/` owns the seam. | code-map, rust, orchestrator | 2026-09-29 |
| [frontend-module-map.md](frontend-module-map.md) | `src/` module by module: nine hand-persisted Zustand stores, the component tree, the normalizer layer, Monaco's split. | code-map, react, zustand | 2026-09-29 |

## Categories

- **entry point**: code-map-overview.md
- **per-side detail**: backend-module-map.md, frontend-module-map.md

## Recent Changes

- 2026-09-29: All three articles created from the `2b35623` scan
