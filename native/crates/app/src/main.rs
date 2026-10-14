//! VS-Monitor native IDE shell (Phase 0 spike + Phase 1 editor shell).
//!
//! Usage: `vs-monitor [FOLDER] [FILE...]` (FILE paths are relative to FOLDER, or absolute inside it;
//! anything outside FOLDER is refused).

mod commands;
mod language;
mod palette;
mod shell;
mod statusbar;
mod theme;
mod workspace;

use std::path::PathBuf;

use gpui::{
    prelude::*, px, size, App, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions,
};
use gpui_component::Root;

use commands::{CloseTab, CommandPalette, NextTab, PrevTab, QuickOpen, Quit, Save, ToggleSidebar};
use shell::Shell;
use workspace::Workspace;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let folder = args
        .next()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let initial_files: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();

    let workspace = match Workspace::open(&folder) {
        Ok(ws) => ws,
        Err(err) => {
            eprintln!("vs-monitor: cannot open {}: {err}", folder.display());
            std::process::exit(2);
        }
    };

    gpui_platform::application()
        .with_assets(gpui_kit_assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            theme::apply(cx);
            bind_keys(cx);

            let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("VS-Monitor - {}", workspace.name()).into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            cx.open_window(options, move |window, cx| {
                let shell = cx.new(|cx| Shell::new(workspace, window, cx));
                shell.update(cx, |shell, cx| shell.open_paths(initial_files, window, cx));
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .expect("failed to open the main window");
            cx.activate(true);
        });
}

fn bind_keys(cx: &mut App) {
    // `secondary` is Ctrl on Linux/Windows and Cmd on macOS.
    cx.bind_keys([
        KeyBinding::new("secondary-s", Save, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("secondary-p", QuickOpen, None),
        KeyBinding::new("secondary-shift-p", CommandPalette, None),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("ctrl-pagedown", NextTab, None),
        KeyBinding::new("ctrl-pageup", PrevTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
    ]);
}
