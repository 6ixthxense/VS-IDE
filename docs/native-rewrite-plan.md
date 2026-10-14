# Native rewrite plan (Rust + GPUI)

Goal: replace the Electron app with a native Rust app that idles at ~0% CPU, uses far less RAM, and starts fast.

## Stack

- UI: `gpui` (via `gpui-component` 0.7.x, which pins `gpui-pre` 0.3.9) — dock, tree, tabs, code editor (tree-sitter + LSP hooks), virtualized table, command palette.
- Rust stable, Cargo workspace in `native/`.
- License note: Zed's editor/terminal crates are GPL/AGPL — do **not** copy code from them. `gpui` and `gpui-component` are Apache-2.0.

## Targets (measured on Windows, release build)

| Metric | Target |
|---|---|
| Idle CPU | < 1% |
| RAM, 10 files open (excluding LSP servers) | < 120 MB |
| Cold start | < 400 ms |
| Installer | < 25 MB |
| 50k-line file scroll | 60 fps |

## Workspace layout

| Crate | Replaces (Electron) | Key crates |
|---|---|---|
| `app` | `src/renderer/**`, `src/main/main.ts`, `window.ts` | `gpui-component` |
| `core` | config, workbench persistence, editor recovery, shared types | `serde`, `directories` |
| `fs_index` | `src/main/services/fileIndex.ts`, `workspace.ts`, `fs.ts` | `ignore`, `notify`, `notify-debouncer-full` |
| `search` | `src/main/services/search.ts`, quick-open fuzzy | `grep-searcher`, `grep-regex`, `nucleo-matcher` |
| `tasks` | `src/main/services/tasks.ts`, `problems.ts` | `std::process`, process-tree kill |
| `terminal` | `src/main/services/terminal.ts` + xterm | `portable-pty`, `alacritty_terminal` |
| `git` | `src/main/services/git.ts` | `git` CLI |
| `db` | `sqliteAdapter.ts`, `sql.ts`, xlsx export | `rusqlite` (bundled), `rust_xlsxwriter`, `calamine` |
| `sysmon` | `systemMonitor.ts` + `systeminformation` | `sysinfo`, optional `nvml-wrapper` |
| `lsp` | Monaco TS worker | `lsp-types`, JSON-RPC over stdio |

No IPC layer: `src/preload/*`, `shared/types/ipc.ts`, `electronApiFallback.ts` have no native equivalent.

## Phases

0. **Spike (go/no-go)** — GPUI window + gpui-component editor on Windows, 50k-line file, **Thai IME and Thai text rendering**, terminal prototype, measure against targets. If Thai input fails, fall back to Tauri 2 + existing React UI.
1. **Editor MVP** — open folder, file tree, tabs, open/save, dirty state, tree-sitter highlight, quick open, config/theme, layout persistence, unsaved-edit recovery.
2. **Terminal + tasks + problems.**
3. **LSP** — diagnostics, completion, hover, go-to-definition, references, rename; servers start lazily per language.
4. **Search + git + command palette.**
5. **System monitor + database + settings** — sysmon polls only while visible (CPU/RAM 2 s, disk/processes 15 s, GPU name once).
6. **Packaging + release** — Velopack installer/updater, CI.

The Electron app is feature-frozen (bug fixes only) until the native app reaches parity. Behavioural tests in `src/test/main/*` are ported to Rust tests in the matching crate.
