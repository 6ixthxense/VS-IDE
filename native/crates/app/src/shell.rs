//! The main window view: activity bar, explorer, tabbed editors, status bar and
//! the Quick Open / command palette overlay.
//!
//! All state lives here and changes only in response to user input (key, mouse)
//! or palette events. There are no timers, pollers or background tasks, so an
//! unfocused/idle window never repaints. (gpui-component's caret blink is the
//! one library-owned timer: it only runs while an editor has focus.)

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui::{
    div, prelude::*, px, transparent_black, App, Context, Entity, FocusHandle, ScrollStrategy,
    SharedString, Subscription, Window,
};
use gpui_component::{
    input::{Editor, EditorState, InputEvent},
    list::ListItem,
    resizable::{h_resizable, resizable_panel},
    tree::{Tree, TreeEntry, TreeItem, TreeState},
    ActiveTheme as _, Icon, IconName, StyledExt as _,
};

use crate::{
    commands::{
        Command, CommandPalette, CloseTab, NextTab, PrevTab, QuickOpen, Quit, Save, ToggleSidebar,
    },
    language::{self, Lang},
    palette::{Palette, PaletteEvent, PaletteKind},
    statusbar::{DocInfo, StatusBar},
    workspace::{FileEntry, Scan, Workspace},
};

struct Tab {
    rel: String,
    name: SharedString,
    lang: Lang,
    editor: Entity<EditorState>,
    dirty: bool,
    crlf: bool,
    _change: Subscription,
}

pub struct Shell {
    ws: Workspace,
    /// Relative paths of all files (for Quick Open).
    files: Rc<Vec<String>>,
    /// Relative paths of directories (tree rows that must not open as files).
    dirs: Rc<HashSet<String>>,
    truncated: bool,
    tree: Entity<TreeState>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    /// Tab (by path) that asked to be closed while dirty; a second close confirms.
    pending_close: Option<String>,
    /// Most recently used files, newest first.
    mru: Vec<String>,
    palette: Option<(Entity<Palette>, Subscription)>,
    sidebar_visible: bool,
    status: Entity<StatusBar>,
    focus: FocusHandle,
}

impl Shell {
    pub fn new(ws: Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scan = ws.scan();
        let (items, files, dirs) = index(&scan);
        let tree = cx.new(|cx| TreeState::new(cx).items(items));
        let status = cx.new(|_| StatusBar::new(ws.name()));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            files: Rc::new(files),
            dirs: Rc::new(dirs),
            truncated: scan.truncated,
            ws,
            tree,
            tabs: Vec::new(),
            active: None,
            pending_close: None,
            mru: Vec::new(),
            palette: None,
            sidebar_visible: true,
            status,
            focus,
        }
    }

    // ---------------------------------------------------------------- state

    fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|ix| self.tabs.get(ix))
    }

    fn notice(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        let text = text.map(SharedString::from);
        self.status.update(cx, |s, cx| s.set_notice(text, cx));
    }

    fn sync_status(&mut self, cx: &mut Context<Self>) {
        let doc = self.active_tab().map(|t| {
            (
                DocInfo {
                    path: t.rel.clone().into(),
                    language: t.lang.label.into(),
                    dirty: t.dirty,
                    crlf: t.crlf,
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

    /// Re-scan the workspace (Explorer: Refresh). Synchronous in this spike;
    /// `vsm-fs_index` should provide an async/incremental version.
    fn refresh_explorer(&mut self, cx: &mut Context<Self>) {
        let scan = self.ws.scan();
        let (items, files, dirs) = index(&scan);
        self.files = Rc::new(files);
        self.dirs = Rc::new(dirs);
        self.truncated = scan.truncated;
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        self.notice(Some("Explorer refreshed".into()), cx);
        cx.notify();
    }

    // ----------------------------------------------------------------- tabs

    pub fn open_file(&mut self, rel: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t.rel == rel) {
            self.activate(ix, window, cx);
            return;
        }
        let loaded = match self.ws.read_text(rel) {
            Ok(loaded) => loaded,
            Err(err) => {
                self.notice(Some(format!("Cannot open {rel}: {err}")), cx);
                return;
            }
        };
        let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
        let lang = language::detect(&name);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(lang.id)
                .default_value(loaded.text)
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
            rel: rel.to_string(),
            name: name.into(),
            lang,
            editor,
            dirty: false,
            crlf: loaded.crlf,
            _change: change,
        });
        let ix = self.tabs.len() - 1;
        self.activate(ix, window, cx);
    }

    fn mark_dirty(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| &t.editor == editor) else {
            return;
        };
        if !self.tabs[ix].dirty {
            self.tabs[ix].dirty = true;
            if self.active == Some(ix) {
                self.status.update(cx, |s, cx| s.set_dirty(true, cx));
            }
            cx.notify();
        }
        self.pending_close = None;
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.active = Some(ix);
        self.pending_close = None;
        let rel = self.tabs[ix].rel.clone();
        self.mru.retain(|r| r != &rel);
        self.mru.insert(0, rel.clone());
        self.sync_status(cx);
        self.notice(None, cx);
        self.reveal_in_tree(&rel, cx);
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn reveal_in_tree(&mut self, rel: &str, cx: &mut Context<Self>) {
        let id: SharedString = rel.to_string().into();
        self.tree.update(cx, |tree, cx| {
            tree.reveal_item(&id, ScrollStrategy::Center, cx);
            let ix = tree.index_of(&id);
            tree.set_selected_index(ix, cx);
        });
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else { return };
        if tab.dirty && self.pending_close.as_deref() != Some(tab.rel.as_str()) {
            self.pending_close = Some(tab.rel.clone());
            let msg = format!("{} has unsaved changes: save (Ctrl+S) or close again to discard", tab.name);
            self.notice(Some(msg), cx);
            return;
        }
        let closed = self.tabs.remove(ix);
        self.mru.retain(|r| r != &closed.rel);
        self.pending_close = None;
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
        self.notice(None, cx);
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

    fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.active else { return };
        let tab = &self.tabs[ix];
        let text = tab.editor.read(cx).value();
        match self.ws.write_text(&tab.rel, &text, tab.crlf) {
            Ok(()) => {
                let msg = format!("Saved {}", tab.name);
                self.tabs[ix].dirty = false;
                self.pending_close = None;
                self.status.update(cx, |s, cx| s.set_dirty(false, cx));
                self.notice(Some(msg), cx);
                cx.notify();
            }
            Err(err) => {
                let msg = format!("Save failed: {err}");
                self.notice(Some(msg), cx);
            }
        }
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
            Command::Stub(what) => {
                self.notice(Some(format!("Not implemented yet: {what}")), cx);
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
        let buttons = entries.into_iter().enumerate().map(|(ix, (icon, stub, cmd))| {
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

        let tree = Tree::new(&self.tree, move |ix, entry: &TreeEntry, _selected, _window, _cx| {
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
        });

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
            .child(div().flex_1().min_h_0().child(tree))
    }

    fn render_tab(&self, ix: usize, tab: &Tab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let active = self.active == Some(ix);
        let (border, primary, warning) = (theme.border, theme.primary, theme.warning);
        let hover_bg = theme.secondary_hover;
        let fg = if active { theme.tab_active_foreground } else { theme.tab_foreground };
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
            .child(div().w(px(140.)).text_right().text_color(theme.foreground).child(keys))
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
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
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

/// Build tree items plus the flat file list and directory set from a scan.
fn index(scan: &Scan) -> (Vec<TreeItem>, Vec<String>, HashSet<String>) {
    let mut by_parent: HashMap<&str, Vec<&FileEntry>> = HashMap::new();
    for entry in &scan.entries {
        let parent = entry.rel.rsplit_once('/').map_or("", |(p, _)| p);
        by_parent.entry(parent).or_default().push(entry);
    }

    fn build(parent: &str, by_parent: &HashMap<&str, Vec<&FileEntry>>) -> Vec<TreeItem> {
        by_parent
            .get(parent)
            .map(|children| {
                children
                    .iter()
                    .map(|e| {
                        let item = TreeItem::new(e.rel.clone(), e.name().to_string());
                        if e.is_dir {
                            item.children(build(&e.rel, by_parent))
                        } else {
                            item
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    let mut files: Vec<String> = scan.files().map(|e| e.rel.clone()).collect();
    files.sort();
    let dirs = scan
        .entries
        .iter()
        .filter(|e| e.is_dir)
        .map(|e| e.rel.clone())
        .collect();
    (build("", &by_parent), files, dirs)
}
