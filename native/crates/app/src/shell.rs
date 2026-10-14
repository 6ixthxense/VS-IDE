//! The main window view: activity bar, explorer, tabbed editors, status bar and
//! the Quick Open / command palette overlay.
//!
//! All state lives here and changes only in response to user input (key, mouse),
//! palette events or the completion of a one-shot background job (workspace
//! scan, file read, Quick Open ranking). There are no timers, pollers or
//! animation requests. The one library-owned timer is gpui-component's caret
//! blink (500 ms): it runs only while an editor has keyboard focus, and the
//! shell moves focus off the editor whenever the window is deactivated so a
//! background window does not repaint (see `Shell::on_activation_changed`).
//!
//! Data-loss guards (all without timers): closing a dirty tab, quitting, closing
//! the window and reloading from disk each need a second request within
//! [`CONFIRM_WINDOW`]; saving refuses to overwrite a file that changed on disk
//! unless Ctrl+S is pressed a second time.

use std::{
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    div, prelude::*, px, transparent_black, App, Context, Entity, FocusHandle, ScrollStrategy,
    SharedString, Subscription, Task, Window,
};
use gpui_component::{
    input::{Editor, EditorState, InputEvent},
    list::ListItem,
    resizable::{h_resizable, resizable_panel},
    tree::{Tree, TreeEntry, TreeEvent, TreeItem, TreeState},
    ActiveTheme as _, Icon, IconName, StyledExt as _,
};

use crate::{
    commands::{
        CloseTab, Command, CommandPalette, NextTab, PrevTab, QuickOpen, Quit, Save, ToggleSidebar,
    },
    language::{self, Lang},
    palette::{Palette, PaletteEvent, PaletteKind},
    statusbar::{DocInfo, StatusBar},
    workspace::{
        self, rel_key, scan_dir, text_hash, DocFormat, FileEntry, FileStamp, LoadedText, SaveError,
        Scan, Workspace,
    },
};

/// How long a "press again to confirm" request stays armed.
const CONFIRM_WINDOW: Duration = Duration::from_secs(10);

struct Tab {
    /// Canonical workspace-relative path (as first opened).
    rel: String,
    /// `rel_key(rel)`: identity for "is this file already open".
    key: String,
    name: SharedString,
    lang: Lang,
    editor: Entity<EditorState>,
    dirty: bool,
    format: DocFormat,
    /// On-disk state at open / last save, to detect external changes.
    stamp: FileStamp,
    /// Hash of the buffer text at open / last save (unchanged-buffer check).
    clean_hash: u64,
    _change: Subscription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Close,
    Overwrite,
    Reload,
    Quit,
}

/// An armed "do it again to confirm" request.
struct Pending {
    kind: PendingKind,
    key: String,
    at: Instant,
}

/// Lifetime of the status-bar message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    /// Plain information; dropped when the user moves to another tab.
    Info,
    /// Belongs to an armed [`Pending`]; dropped with it.
    Pending,
    /// A failure (cannot open, save failed...); kept until the next notice or edit.
    Error,
}

/// Everything the background walk produces (all `Send`).
struct Indexed {
    scan: Scan,
    files: Vec<String>,
    dirs: HashSet<String>,
}

pub struct Shell {
    ws: Workspace,
    /// Relative paths of all files (Quick Open); shared with background ranking.
    files: Arc<Vec<String>>,
    /// Relative paths of directories (tree rows that must not open as files).
    dirs: Rc<HashSet<String>>,
    truncated: bool,
    /// A workspace scan is in flight.
    loading: bool,
    scan_task: Option<Task<()>>,
    /// Folders the user has expanded; survives Explorer refreshes.
    expanded: HashSet<String>,
    tree: Entity<TreeState>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    /// Keys of files currently being read, to ignore duplicate open requests.
    opening: HashSet<String>,
    pending: Option<Pending>,
    notice_kind: NoticeKind,
    /// Most recently used files, newest first.
    mru: Vec<String>,
    palette: Option<(Entity<Palette>, Subscription)>,
    sidebar_visible: bool,
    status: Entity<StatusBar>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(ws: Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let status = cx.new(|_| StatusBar::new(ws.name()));
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        let me = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            me.update(cx, |this, cx| this.guard_quit(cx))
                .unwrap_or(true)
        });
        let subscriptions = vec![
            cx.observe_window_activation(window, |this, window, cx| {
                this.on_activation_changed(window, cx)
            }),
            cx.subscribe(&tree, |this, _, event: &TreeEvent, _| match event {
                TreeEvent::Expanded(id) => {
                    this.expanded.insert(id.to_string());
                }
                TreeEvent::Collapsed(id) => {
                    this.expanded.remove(id.as_str());
                }
            }),
        ];

        let mut this = Self {
            ws,
            files: Arc::new(Vec::new()),
            dirs: Rc::new(HashSet::new()),
            truncated: false,
            loading: false,
            scan_task: None,
            expanded: HashSet::new(),
            tree,
            tabs: Vec::new(),
            active: None,
            opening: HashSet::new(),
            pending: None,
            notice_kind: NoticeKind::Info,
            mru: Vec::new(),
            palette: None,
            sidebar_visible: true,
            status,
            focus,
            _subscriptions: subscriptions,
        };
        this.start_scan(false, cx);
        this
    }

    // ---------------------------------------------------------------- state

    fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|ix| self.tabs.get(ix))
    }

    fn set_notice(&mut self, text: impl Into<String>, kind: NoticeKind, cx: &mut Context<Self>) {
        self.notice_kind = kind;
        let text = SharedString::from(text.into());
        self.status.update(cx, |s, cx| s.set_notice(Some(text), cx));
    }

    fn clear_notice(&mut self, cx: &mut Context<Self>) {
        self.notice_kind = NoticeKind::Info;
        self.status.update(cx, |s, cx| s.set_notice(None, cx));
    }

    /// Called when the user moves on (another tab, a close): drops the previous
    /// tab's pending message and plain info, but keeps errors.
    fn clear_transient(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        if matches!(self.notice_kind, NoticeKind::Pending | NoticeKind::Info) {
            self.clear_notice(cx);
        }
    }

    /// Arm `kind` for `key` and return `false`, or, if the same request is
    /// already armed (and recent), disarm it and return `true` (= confirmed).
    fn confirm(
        &mut self,
        kind: PendingKind,
        key: &str,
        message: String,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.take_pending(kind, key) {
            self.clear_notice(cx);
            return true;
        }
        self.pending = Some(Pending {
            kind,
            key: key.to_string(),
            at: Instant::now(),
        });
        self.set_notice(message, NoticeKind::Pending, cx);
        false
    }

    /// Disarm `kind` for `key`; `true` if it was armed and still fresh.
    fn take_pending(&mut self, kind: PendingKind, key: &str) -> bool {
        let hit = self
            .pending
            .as_ref()
            .is_some_and(|p| p.kind == kind && p.key == key && p.at.elapsed() < CONFIRM_WINDOW);
        if hit {
            self.pending = None;
        }
        hit
    }

    fn sync_status(&mut self, cx: &mut Context<Self>) {
        let doc = self.active_tab().map(|t| {
            (
                DocInfo {
                    path: t.rel.clone().into(),
                    language: t.lang.label.into(),
                    dirty: t.dirty,
                    crlf: t.format.eol == workspace::LineEnding::Crlf,
                },
                t.editor.clone(),
            )
        });
        self.status.update(cx, |s, cx| s.set_document(doc, cx));
    }

    fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_tab().map(|t| t.editor.clone()) {
            Some(editor) => editor.update(cx, |state, cx| state.focus(window, cx)),
            None => self.focus.focus(window, cx),
        }
    }

    /// Background windows must not blink a caret (that is a 2 Hz repaint):
    /// park focus on the shell while inactive, restore it on activation.
    fn on_activation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            self.focus.focus(window, cx);
        } else if let Some((palette, _)) = &self.palette {
            palette.update(cx, |p, cx| p.focus(window, cx));
        } else {
            self.focus_editor(window, cx);
        }
    }

    // ------------------------------------------------------------- explorer

    /// Walk the workspace on the background executor. The tree shows a loading
    /// state until the result arrives; a newer scan replaces an older one.
    fn start_scan(&mut self, announce: bool, cx: &mut Context<Self>) {
        self.loading = true;
        let root: PathBuf = self.ws.root().to_path_buf();
        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let indexed = cx
                .background_spawn(async move {
                    let scan = scan_dir(&root);
                    let mut files: Vec<String> = scan.files().map(|e| e.rel.clone()).collect();
                    files.sort();
                    let dirs = scan
                        .entries
                        .iter()
                        .filter(|e| e.is_dir)
                        .map(|e| e.rel.clone())
                        .collect();
                    Indexed { scan, files, dirs }
                })
                .await;
            this.update(cx, |this, cx| this.apply_scan(indexed, announce, cx))
                .ok();
        }));
        cx.notify();
    }

    fn apply_scan(&mut self, indexed: Indexed, announce: bool, cx: &mut Context<Self>) {
        let Indexed { scan, files, dirs } = indexed;
        let items = build_items(&scan, &self.expanded);
        self.files = Arc::new(files);
        self.dirs = Rc::new(dirs);
        self.truncated = scan.truncated;
        self.loading = false;
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        // Keep the active file highlighted (without expanding anything).
        if let Some(rel) = self.active_tab().map(|t| t.rel.clone()) {
            self.select_in_tree(&rel, cx);
        }
        if announce {
            self.set_notice("Explorer refreshed", NoticeKind::Info, cx);
        }
        cx.notify();
    }

    fn refresh_explorer(&mut self, cx: &mut Context<Self>) {
        self.start_scan(true, cx);
    }

    fn select_in_tree(&mut self, rel: &str, cx: &mut Context<Self>) {
        let id: SharedString = rel.to_string().into();
        self.tree.update(cx, |tree, cx| {
            let ix = tree.index_of(&id);
            tree.set_selected_index(ix, cx);
        });
    }

    fn reveal_in_tree(&mut self, rel: &str, cx: &mut Context<Self>) {
        let id: SharedString = rel.to_string().into();
        self.tree.update(cx, |tree, cx| {
            tree.reveal_item(&id, ScrollStrategy::Center, cx);
            let ix = tree.index_of(&id);
            tree.set_selected_index(ix, cx);
        });
    }

    // ----------------------------------------------------------------- tabs

    pub fn open_file(&mut self, rel: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_paths(vec![rel.to_string()], window, cx);
    }

    /// Open `paths` in order (relative to the workspace root, or absolute paths
    /// inside it). Each path is normalised and checked to be inside the
    /// workspace; the file is read on a background thread, so the UI never
    /// blocks on disk. Failures become status-bar errors and do not stop the
    /// remaining paths.
    pub fn open_paths(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let mut queue = paths.into_iter();
        while let Some(raw) = queue.next() {
            let rel = match self.ws.normalize(&raw) {
                Ok(rel) => rel,
                Err(err) => {
                    self.set_notice(format!("Cannot open {raw}: {err}"), NoticeKind::Error, cx);
                    continue;
                }
            };
            let key = rel_key(&rel);
            if let Some(ix) = self.tabs.iter().position(|t| t.key == key) {
                self.activate(ix, window, cx);
                continue;
            }
            if !self.opening.insert(key.clone()) {
                continue; // already being read
            }
            let rest: Vec<String> = queue.collect();
            let abs = self.ws.abs(&rel);
            self.spawn_load(abs, window, cx, move |this, loaded, window, cx| {
                this.opening.remove(&key);
                match loaded {
                    Ok(loaded) => this.add_tab(rel, key, loaded, window, cx),
                    Err(err) => {
                        this.set_notice(format!("Cannot open {rel}: {err}"), NoticeKind::Error, cx)
                    }
                }
                // Continue with the remaining paths, preserving their order.
                this.open_paths(rest, window, cx);
            });
            return;
        }
    }

    /// Read + decode `path` on the background executor, then run `done` on the UI.
    fn spawn_load(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, io::Result<LoadedText>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { workspace::load(&path) })
                .await;
            this.update_in(cx, |this, window, cx| done(this, loaded, window, cx))
                .ok();
        })
        .detach();
    }

    fn add_tab(
        &mut self,
        rel: String,
        key: String,
        loaded: LoadedText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.tabs.iter().position(|t| t.key == key) {
            self.activate(ix, window, cx);
            return;
        }
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        let lang = language::detect(&name);
        let text = loaded.text;
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(lang.id)
                .default_value(text)
        });
        let change = cx.subscribe_in(
            &editor,
            window,
            |this: &mut Self, editor, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mark_dirty(editor, cx);
                }
            },
        );
        self.tabs.push(Tab {
            rel,
            key,
            name: name.into(),
            lang,
            editor,
            dirty: false,
            format: loaded.format,
            stamp: loaded.stamp,
            clean_hash: loaded.hash,
            _change: change,
        });
        let ix = self.tabs.len() - 1;
        self.activate(ix, window, cx);
    }

    fn mark_dirty(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| &t.editor == editor) else {
            return;
        };
        // Any edit disarms pending confirmations and acknowledges old errors.
        self.pending = None;
        if matches!(self.notice_kind, NoticeKind::Pending | NoticeKind::Error) {
            self.clear_notice(cx);
        }
        if !self.tabs[ix].dirty {
            self.tabs[ix].dirty = true;
            if self.active == Some(ix) {
                self.status.update(cx, |s, cx| s.set_dirty(true, cx));
            }
            cx.notify();
        }
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.active = Some(ix);
        let rel = self.tabs[ix].rel.clone();
        self.mru.retain(|r| r != &rel);
        self.mru.insert(0, rel.clone());
        self.sync_status(cx);
        self.clear_transient(cx);
        self.reveal_in_tree(&rel, cx);
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn dirty_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.dirty).count()
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else { return };
        if tab.dirty {
            let msg = format!(
                "{} has unsaved changes: save (Ctrl+S) or close again to discard",
                tab.name
            );
            let key = tab.key.clone();
            if !self.confirm(PendingKind::Close, &key, msg, cx) {
                return;
            }
        }
        let closed = self.tabs.remove(ix);
        self.mru.retain(|r| r != &closed.rel);
        self.active = if self.tabs.is_empty() {
            None
        } else {
            // Prefer the most recently used remaining tab.
            self.mru
                .iter()
                .find_map(|r| self.tabs.iter().position(|t| &t.rel == r))
                .or(Some(ix.min(self.tabs.len() - 1)))
        };
        self.sync_status(cx);
        self.clear_transient(cx);
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn cycle_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.tabs.len() as isize;
        if len < 2 {
            return;
        }
        let cur = self.active.unwrap_or(0) as isize;
        self.activate((cur + delta).rem_euclid(len) as usize, window, cx);
    }

    /// One guard for every way out of the app (Quit action, palette command,
    /// window close button). Returns `true` when it is fine to exit now.
    fn guard_quit(&mut self, cx: &mut Context<Self>) -> bool {
        let dirty = self.dirty_count();
        if dirty == 0 {
            return true;
        }
        let msg = format!(
            "{dirty} file(s) with unsaved changes: save them (Ctrl+S), or quit again to discard"
        );
        self.confirm(PendingKind::Quit, "", msg, cx)
    }

    // ------------------------------------------------------- save / reload

    fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.active else { return };
        let tab = &self.tabs[ix];
        let (key, rel, name) = (tab.key.clone(), tab.rel.clone(), tab.name.clone());
        // A second Ctrl+S right after a conflict notice forces the overwrite.
        let force = self.take_pending(PendingKind::Overwrite, &key);
        let tab = &self.tabs[ix];
        let text = tab.editor.read(cx).value();
        let hash = text_hash(&text);

        if !force && hash == tab.clean_hash {
            // Buffer equals what was loaded/saved: leave the file byte-identical.
            self.tabs[ix].dirty = false;
            self.status.update(cx, |s, cx| s.set_dirty(false, cx));
            self.set_notice(
                format!("No changes to save in {name}"),
                NoticeKind::Info,
                cx,
            );
            cx.notify();
            return;
        }

        match self.ws.save(&rel, &text, &tab.format, &tab.stamp, force) {
            Ok(stamp) => {
                let tab = &mut self.tabs[ix];
                tab.stamp = stamp;
                tab.clean_hash = hash;
                tab.dirty = false;
                self.pending = None;
                self.status.update(cx, |s, cx| s.set_dirty(false, cx));
                self.set_notice(format!("Saved {name}"), NoticeKind::Info, cx);
                cx.notify();
            }
            Err(SaveError::Conflict) => {
                self.pending = None;
                let msg = format!(
                    "{name} changed on disk: Ctrl+S again to overwrite, or run \"File: Reload from Disk\""
                );
                self.confirm(PendingKind::Overwrite, &key, msg, cx);
            }
            Err(err) => {
                // A failed save must not leave a close/quit confirmation armed.
                self.pending = None;
                self.set_notice(format!("Save failed: {err}"), NoticeKind::Error, cx);
            }
        }
    }

    fn reload_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        let (key, rel, name, dirty) = (
            tab.key.clone(),
            tab.rel.clone(),
            tab.name.clone(),
            tab.dirty,
        );
        if dirty {
            let msg = format!("{name} has unsaved changes: reload again to discard them");
            if !self.confirm(PendingKind::Reload, &key, msg, cx) {
                return;
            }
        }
        let abs = self.ws.abs(&rel);
        self.spawn_load(abs, window, cx, move |this, loaded, window, cx| {
            let Some(ix) = this.tabs.iter().position(|t| t.key == key) else {
                return;
            };
            match loaded {
                Ok(loaded) => {
                    let text = loaded.text;
                    let tab = &mut this.tabs[ix];
                    tab.editor
                        .update(cx, |state, cx| state.set_value(text, window, cx));
                    tab.format = loaded.format;
                    tab.stamp = loaded.stamp;
                    tab.clean_hash = loaded.hash;
                    tab.dirty = false;
                    this.pending = None;
                    if this.active == Some(ix) {
                        this.sync_status(cx);
                    }
                    this.set_notice(format!("Reloaded {name} from disk"), NoticeKind::Info, cx);
                    cx.notify();
                }
                Err(err) => this.set_notice(format!("Reload failed: {err}"), NoticeKind::Error, cx),
            }
        });
    }

    // -------------------------------------------------------------- palette

    fn open_palette(&mut self, kind: PaletteKind, window: &mut Window, cx: &mut Context<Self>) {
        let same_kind_open = self
            .palette
            .as_ref()
            .is_some_and(|(p, _)| p.read(cx).kind() == kind);
        if same_kind_open {
            self.close_palette(window, cx);
            return;
        }
        let files = self.files.clone();
        let recent = self.mru.clone();
        let palette = cx.new(|cx| Palette::new(kind, files, recent, window, cx));
        let sub = cx.subscribe_in(
            &palette,
            window,
            |this: &mut Self, _, event: &PaletteEvent, window, cx| {
                this.close_palette(window, cx);
                match event {
                    PaletteEvent::Open(rel) => this.open_file(rel, window, cx),
                    PaletteEvent::Run(cmd) => this.run_command(*cmd, window, cx),
                    PaletteEvent::Dismiss => {}
                }
            },
        );
        self.palette = Some((palette, sub));
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.focus_editor(window, cx);
            cx.notify();
        }
    }

    pub fn run_command(&mut self, cmd: Command, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            Command::Save => self.save_active(cx),
            Command::CloseTab => {
                if let Some(ix) = self.active {
                    self.close_tab(ix, window, cx);
                }
            }
            Command::NextTab => self.cycle_tab(1, window, cx),
            Command::PrevTab => self.cycle_tab(-1, window, cx),
            Command::QuickOpen => self.open_palette(PaletteKind::QuickOpen, window, cx),
            Command::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                cx.notify();
            }
            Command::RefreshExplorer => self.refresh_explorer(cx),
            Command::ReloadFromDisk => self.reload_active(window, cx),
            Command::Quit => {
                if self.guard_quit(cx) {
                    cx.quit();
                }
            }
            Command::Stub(what) => {
                self.set_notice(format!("Not implemented yet: {what}"), NoticeKind::Info, cx);
            }
        }
    }

    // --------------------------------------------------------------- render

    fn render_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (fg, dim, primary) = (theme.foreground, theme.muted_foreground, theme.primary);
        let entries: [(IconName, &'static str, Option<Command>); 6] = [
            (IconName::Folder, "", Some(Command::ToggleSidebar)),
            (IconName::Search, "Search (vsm-search)", None),
            (IconName::Github, "Source control (vsm-git)", None),
            (IconName::SquareTerminal, "Terminal (vsm-terminal)", None),
            (IconName::Cpu, "System monitor (vsm-sysmon)", None),
            (IconName::HardDrive, "Database (vsm-db)", None),
        ];
        let buttons = entries
            .into_iter()
            .enumerate()
            .map(|(ix, (icon, stub, cmd))| {
                let active = cmd.is_some() && self.sidebar_visible;
                div()
                    .id(("activity", ix))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size(px(48.))
                    .cursor_pointer()
                    .border_l_2()
                    .border_color(if active { primary } else { transparent_black() })
                    .text_color(if active { fg } else { dim })
                    .hover(|style| style.text_color(fg))
                    .child(Icon::new(icon).size_5())
                    .on_click(cx.listener(move |this, _, window, cx| match cmd {
                        Some(cmd) => this.run_command(cmd, window, cx),
                        None => this.run_command(Command::Stub(stub), window, cx),
                    }))
            });
        div()
            .v_flex()
            .flex_none()
            .w(px(48.))
            .h_full()
            .bg(gpui::rgb(0x141414))
            .children(buttons)
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let shell = cx.entity().downgrade();
        let dirs = self.dirs.clone();
        let muted = theme.muted_foreground;
        let fg = theme.sidebar_foreground;

        let tree = Tree::new(
            &self.tree,
            move |ix, entry: &TreeEntry, _selected, _window, _cx| {
                let rel = entry.item().id.clone();
                let is_dir = dirs.contains(rel.as_str());
                let icon = if is_dir {
                    if entry.is_expanded() {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    }
                } else {
                    IconName::File
                };
                let shell = shell.clone();
                ListItem::new(ix)
                    .pl(px(8. + 12. * entry.depth() as f32))
                    .pr_2()
                    .py(px(2.))
                    .text_sm()
                    .text_color(fg)
                    .child(
                        div()
                            .h_flex()
                            .gap_1p5()
                            .min_w_0()
                            .child(Icon::new(icon).size_4().text_color(muted).flex_none())
                            .child(div().truncate().child(entry.item().label.clone())),
                    )
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        if is_dir {
                            return;
                        }
                        let rel = rel.clone();
                        let _ = shell.update(cx, |this, cx| this.open_file(&rel, window, cx));
                    })
            },
        );

        div()
            .v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                div()
                    .h_flex()
                    .flex_none()
                    .h(px(35.))
                    .px_3()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_none().child("EXPLORER"))
                    .child(
                        div()
                            .truncate()
                            .text_color(theme.foreground)
                            .child(self.ws.name().to_uppercase()),
                    ),
            )
            .when(self.truncated, |this| {
                this.child(
                    div()
                        .px_3()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.warning)
                        .child("Folder is large; listing truncated"),
                )
            })
            .when(self.loading && self.files.is_empty(), |this| {
                this.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_xs()
                        .text_color(muted)
                        .child("Loading..."),
                )
            })
            .child(div().flex_1().min_h_0().child(tree))
    }

    fn render_tab(&self, ix: usize, tab: &Tab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let active = self.active == Some(ix);
        let (border, primary, warning) = (theme.border, theme.primary, theme.warning);
        let hover_bg = theme.secondary_hover;
        let fg = if active {
            theme.tab_active_foreground
        } else {
            theme.tab_foreground
        };
        div()
            .id(("tab", ix))
            .h_flex()
            .flex_none()
            .gap_2()
            .h_full()
            .px_3()
            .min_w(px(110.))
            .max_w(px(220.))
            .cursor_pointer()
            .relative()
            .border_r_1()
            .border_color(border)
            .when(active, |this| {
                this.bg(theme.tab_active).child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .w_full()
                        .h(px(2.))
                        .bg(primary),
                )
            })
            .text_color(fg)
            .text_xs()
            .child(
                div()
                    .h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().truncate().child(tab.name.clone()))
                    .when(tab.dirty, |this| {
                        this.child(div().flex_none().text_color(warning).child("\u{25CF}"))
                    }),
            )
            .child(
                div()
                    .id(("tab-close", ix))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size_4()
                    .rounded(px(4.))
                    .hover(move |style| style.bg(hover_bg))
                    .child(Icon::new(IconName::Close).size_3())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_tab(ix, window, cx);
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
    }

    fn render_editor_area(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (bg, tab_bar, border) = {
            let theme = cx.theme();
            (theme.background, theme.tab_bar, theme.border)
        };
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| self.render_tab(ix, tab, cx))
            .collect::<Vec<_>>();

        let body = match self.active_tab() {
            Some(tab) => Editor::new(&tab.editor)
                .bordered(false)
                .size_full()
                .into_any_element(),
            None => welcome(cx).into_any_element(),
        };

        div()
            .v_flex()
            .size_full()
            .min_w_0()
            .bg(bg)
            .child(
                div()
                    .id("tab-bar")
                    .h_flex()
                    .flex_none()
                    .h(px(35.))
                    .w_full()
                    .bg(tab_bar)
                    .border_b_1()
                    .border_color(border)
                    .overflow_x_scroll()
                    .children(tabs),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

fn welcome(cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let row = |keys: &'static str, what: &'static str| {
        div()
            .h_flex()
            .gap_3()
            .child(
                div()
                    .w(px(140.))
                    .text_right()
                    .text_color(theme.foreground)
                    .child(keys),
            )
            .child(div().child(what))
    };
    div()
        .v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            div()
                .text_2xl()
                .text_color(theme.foreground.opacity(0.4))
                .child("VS-Monitor"),
        )
        .child(row("Ctrl+P", "Quick Open"))
        .child(row("Ctrl+Shift+P", "Command Palette"))
        .child(row("Ctrl+S", "Save"))
        .child(row("Ctrl+W", "Close Editor"))
        .child(row("Ctrl+B", "Toggle Sidebar"))
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = (theme.background, theme.foreground, theme.border);

        let editor_area = self.render_editor_area(cx);
        let main = if self.sidebar_visible {
            let sidebar = self.render_sidebar(cx);
            h_resizable("main-split")
                .child(
                    resizable_panel()
                        .size(px(260.))
                        .size_range(px(140.)..px(640.))
                        .child(sidebar),
                )
                .child(resizable_panel().child(editor_area))
                .into_any_element()
        } else {
            editor_area.into_any_element()
        };

        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .relative()
            .v_flex()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save_active(cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.run_command(Command::CloseTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| this.cycle_tab(-1, window, cx)))
            .on_action(cx.listener(|this, _: &QuickOpen, window, cx| {
                this.open_palette(PaletteKind::QuickOpen, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CommandPalette, window, cx| {
                this.open_palette(PaletteKind::Commands, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_visible = !this.sidebar_visible;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                if this.guard_quit(cx) {
                    cx.quit();
                }
            }))
            .child(
                div()
                    .h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.render_activity_bar(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .border_l_1()
                            .border_color(border)
                            .child(main),
                    ),
            )
            .child(self.status.clone())
            .when_some(self.palette.as_ref(), |this, (palette, _)| {
                this.child(palette.clone())
            })
    }
}

/// Build the tree from a scan; folders in `expanded` start open.
fn build_items(scan: &Scan, expanded: &HashSet<String>) -> Vec<TreeItem> {
    let mut by_parent: HashMap<&str, Vec<&FileEntry>> = HashMap::new();
    for entry in &scan.entries {
        let parent = entry.rel.rsplit_once('/').map_or("", |(p, _)| p);
        by_parent.entry(parent).or_default().push(entry);
    }

    fn build(
        parent: &str,
        by_parent: &HashMap<&str, Vec<&FileEntry>>,
        expanded: &HashSet<String>,
    ) -> Vec<TreeItem> {
        by_parent
            .get(parent)
            .map(|children| {
                children
                    .iter()
                    .map(|e| {
                        let item = TreeItem::new(e.rel.clone(), e.name().to_string());
                        if e.is_dir {
                            item.expanded(expanded.contains(&e.rel))
                                .children(build(&e.rel, by_parent, expanded))
                        } else {
                            item
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    build("", &by_parent, expanded)
}
