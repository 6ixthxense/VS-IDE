# vsm-app (`vs-monitor`)

Native GPUI front end for VS-Monitor. This is the Phase 0 spike plus the Phase 1
editor shell from `docs/native-rewrite-plan.md`.

Built on `gpui-component` 0.7.3 (which pins `gpui-pre` 0.3.9, i.e. GPUI).

## Run

```sh
cd native
cargo run -p vsm-app -- [FOLDER] [FILE...]    # FOLDER defaults to ".", FILE paths are relative to FOLDER
cargo build -p vsm-app --release
```

Linux build dependencies (Debian/Ubuntu):

```sh
sudo apt-get install -y libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libvulkan-dev libx11-xcb-dev libxcb-xkb-dev libfontconfig-dev libasound2-dev \
  mesa-vulkan-drivers   # software Vulkan (lavapipe) when there is no GPU
```

## What works

- Window with activity bar, resizable Explorer sidebar, tabbed editors, status bar.
- Explorer: tree of the opened folder (skips `.git`, `node_modules`, `target`);
  click a file to open it, click a folder to expand/collapse, arrow keys navigate.
- Editor: gpui-component code editor with tree-sitter highlighting for
  TypeScript/TSX/JavaScript/Python/Rust/JSON/Markdown/TOML/YAML/HTML/CSS/Bash/Go/C/C++/Java/SQL,
  line numbers, indent guides, code folding, in-file search (Ctrl+F).
- Tabs: open, switch (click, Ctrl+PageUp/PageDown, Ctrl+Tab), close (x or Ctrl+W),
  dirty dot. Closing a dirty tab asks to press close again to discard.
- Ctrl+S saves atomically (permissions and symlinks preserved; CRLF/LF by majority, BOM kept).
  If the file changed on disk since it was opened, save is refused: press Ctrl+S again to overwrite,
  or run "File: Reload from Disk".
- Quit (Ctrl+Q / window close) with unsaved tabs asks you to repeat it within 10 s to discard. Binary / non-UTF-8 / >32 MB files are refused with a status message.
- Quick Open (Ctrl+P): nucleo ranking (vsm-search) on the background executor; empty query lists recently used tabs first.
- Command palette (Ctrl+Shift+P): fuzzy command list; several entries are stubs
  ("Not implemented yet") marking where backend crates plug in.
- Status bar: workspace, file path, dirty marker, `Ln/Col`, EOL, language.
- Dark theme from the Electron palette (`assets/vsm-dark.json`).

| Key | Action |
|---|---|
| Ctrl+P / Ctrl+Shift+P | Quick Open / Command palette |
| Ctrl+S | Save |
| Ctrl+W | Close editor |
| Ctrl+B | Toggle sidebar |
| Ctrl+PageUp/PageDown, Ctrl+Tab | Previous / next editor |
| Ctrl+Q | Quit |

(`Ctrl` is `Cmd` on macOS.)

## Headless run / screenshot (no GPU)

Verified in a Linux container with software Vulkan:

```sh
Xvfb :99 -screen 0 1280x800x24 &
DISPLAY=:99 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  ./target/debug/vs-monitor crates/app src/main.rs &
DISPLAY=:99 import -window root docs/screenshot.png      # ImageMagick
```

`docs/screenshot.png` was produced this way (Quick Open overlay open).

## Idle behaviour

There are no timers, polling loops or animation requests in this crate; the UI
only redraws in response to input. The one library-owned timer is the caret
blink inside gpui-component's editor (500 ms), which runs only while an editor
has keyboard focus. Measured under Xvfb/lavapipe: with no editor open the
process used ~0 CPU over 10 s; with an editor focused it repaints at the blink rate
(software rendering makes each frame expensive; a GPU makes it negligible).

## Layout

```
src/main.rs       entry: CLI args, theme, key bindings, window
src/shell.rs      Shell view: activity bar, explorer, tabs, editor area, palette plumbing
src/palette.rs    Quick Open / command palette overlay (event driven)
src/statusbar.rs  status bar view (observes the active editor for Ln/Col)
src/commands.rs   gpui actions + palette command list
src/workspace.rs  ALL filesystem access: path normalisation, scan, decode/encode (EOL/BOM),
                  atomic save with conflict detection (uses vsm-core / vsm-fs_index)
src/language.rs   file name -> tree-sitter language id
src/theme.rs      applies assets/vsm-dark.json
```

## Not done yet

Layout persistence, unsaved-edit recovery, file watching, new/rename/delete in
the explorer, split editors, settings UI, LSP, terminal/search/git/sysmon/db panels,
Thai IME verification (needs a Windows run, per the plan's go/no-go list).
