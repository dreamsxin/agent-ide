# Changelog

## Unreleased

Reconciled with the rewritten `main` line (the remote had been force-pushed with its own, more advanced implementations of the same phases — MCP client, permission model, provider-native tool calling — plus CI, a localized UI, the tool loop, browser/computer-use tools, and a much larger test suite). Where both lines implemented the same feature, the `main` implementation won; the unique contributions from this line were kept.

### Added

- `npm run visual:test` (`scripts/visual-check.mjs`) now runs against the localized UI: selectors accept both English and Chinese labels (palette placeholder, permissions section, context-preview button); 14/14 checks pass on the merged tree.
- `visual:test` wired into the CI frontend job: the visual smoke runs on every push/PR in headless Chrome on ubuntu runners (sandboxless fallback added to the launcher for restricted runners), and `test-results/visual/` screenshots upload as a 7-day artifact on both green and red runs. The merge commit `ba6be6e` itself was verified green by the pre-existing three-platform CI before this change.
- CHANGELOG.md (this file), carried over from the v0.2.0 release work.

### Changed

- `charge_image_bytes` migrated from the deprecated `AtomicUsize::fetch_update` to `try_update` (pure rename) — current stable rustc deprecates the old name and CI runs clippy with `-D warnings` on floating stable.
- Merge hygiene: dropped the superseded registrations the textual merge had left in `lib.rs` (duplicate `McpState` manage, `mcp_*`/`permission_policy`/local-inference commands whose implementations were replaced), removed the orphaned RMCP-based `services/permissions.rs` and `tests/mcp_integration.rs`, and restored `playwright-core` after taking the remote `package.json`.

### Superseded by `main`

These were implemented on this line and then replaced by the `main` implementations during the merge (our versions remain on branch `evolution/local-2026-10` for reference): the RMCP-SDK MCP client with startup auto-connect, the `services/permissions.rs` ask/suggest/auto policy service, and the `get/set_permission_policy` commands. The `main` line ships its own hand-written stdio MCP client (with argument redaction and tool-name hijack guards), a preset permission model named after what it grants (read-only / create-files / run-commands), and a full agent tool loop.

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
