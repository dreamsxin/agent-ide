# Changelog

## Unreleased

### Added

- Phase 9.0.3 MCP client, first pass: `services/mcp.rs` implements the RMCP stdio transport (spawn → initialize → tool discovery → invocation) with a persisted `mcp.json` config (validated, unique names, required commands), a connection registry managed by Tauri, and startup auto-connect of enabled servers. Discovered tools join the Phase 9.0.1 native tool surface as `mcp__<server>__<tool>`; agent pipeline stages execute MCP tool calls and append fenced `mcp-tool-result` blocks; seven `mcp_*` Tauri commands expose the surface. Validated by live integration tests (`tests/mcp_integration.rs`) driving a real child-process MCP server.
- Phase 9.0.4 desktop permission model, first pass: `services/permissions.rs` defines the persisted permission policy (ask/suggest/auto presets aligned with the frontend preset tables, `edit` kept as a CLI alias, granular create/edit/delete/commands/git flags, deny-path rules, per-run and per-batch caps) and a `--allow-run` style command matcher now shared with the CLI.
- Orchestrator enforcement: Auto-mode auto-apply consults the policy, falls back to the manual review flow with an `auto_apply_blocked` action log on violations, and tracks per-run auto-apply counts (reset on `begin_run`).
- Agent Permissions settings section (Agent panel → Settings): preset chips, five granular toggles (file creation/editing/deletion, command execution, git actions), denied-path and command-pattern editors, per-run cap inputs; loads and saves the backend policy via the new `get_permission_policy` / `set_permission_policy` commands.
- Visual smoke check for the permissions UI (`visual:test`, now 14 checks).

### Changed

- Frontend permission default preset moved from `suggest` to `auto` to preserve the desktop's historical Auto-mode behavior; users can switch to Ask/Suggest for stricter guardrails.
- CLI `--allow-run` matching now delegates to `services::permissions::is_command_allowed` (same exact/prefix/trust-all semantics).

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
