# Changelog

## 0.2.0 — 2026-10-24

First packaged, usable IDE release (Windows NSIS installer, `--no-default-features` build without the pending local-inference engine).

### Added

- Automated visual smoke verification: `npm run visual:test` (`scripts/visual-check.mjs`) drives the Vite web preview in headless Edge/Chrome via `playwright-core`, asserts required layout elements and bottom-tab/command-palette interactions, captures per-state screenshots plus `summary.json` under `test-results/visual/`, and fails on uncaught page errors.
- Vitest coverage for the Phase 9.1/9.2 editor utilities: incremental renderer (dirty-line marking/eviction, viewport tracking, frame-budget rendering, metrics/profiling) and intelligent completion engine (context analysis, suggestion ranking/clamping, cache behavior, Chinese prompt optimizer, Monaco provider mapping and failure fallback).
- Vitest coverage for frontend stores: task run lifecycle and terminal command/session queueing (`useTaskStore`) and log persistence with per-workspace restore (`useLogStore`), growing the frontend suite from 14 to 53 tests.
- Per-run AGENTS.md project-memory toggle in the Chat context preview (`includeProjectMemory` flows through context estimates and Agent runs).
- Windows packaging flag `-NoDefaultFeatures` on `scripts/package-windows.ps1` for release builds on machines without LLVM/libclang; the feature choice is recorded in `manifest.json`.

### Fixed

- Broken `npm run build`: six TypeScript errors in the half-landed Phase 9.1/9.2 files (`useIntelligentCompletion.ts`, `EditorContainer.tsx`); `maxSuggestions`/`onSuggestionGenerated` are now actually wired through the Monaco completion provider.
- Removed the missing `icons/icon.icns` reference from the Tauri bundle config (macOS packaging re-adds it when it ships).

### Verified

- `npm run build` passes (tsc + Vite).
- `npm test`: 53 passed / 0 failed across 10 files.
- `cargo check` and `cargo test --no-default-features`: 127 passed / 0 failed.
- `npm run visual:test`: 13/13 checks passed, 0 uncaught page errors, screenshots visually reviewed.

## 0.1.0 — 2026-04-24

Initial internal prototype: Tauri v2 desktop shell, Monaco editor, Explorer/Git/Terminal panels, Agent planning and diff review scaffolding.
