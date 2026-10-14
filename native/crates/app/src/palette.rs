//! Modal overlay used for both Quick Open (Ctrl+P) and the command palette
//! (Ctrl+Shift+P). One text input, one ranked result list, keyboard driven.
//!
//! The overlay is purely event driven: it re-ranks only when the query text
//! changes and emits [`PaletteEvent`]s that the shell reacts to.

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, KeyDownEvent, MouseButton,
    ScrollHandle, SharedString, Subscription, Task, Window,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    ActiveTheme as _, StyledExt as _,
};

use vsm_search::{build_quick_open_items, FuzzyMatcher};

use crate::commands::{Command, COMMANDS};

const MAX_RESULTS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    QuickOpen,
    Commands,
}

#[derive(Debug, Clone)]
pub enum PaletteEvent {
    /// Open a workspace-relative file.
    Open(String),
    Run(Command),
    Dismiss,
}

impl EventEmitter<PaletteEvent> for Palette {}

#[derive(Clone)]
enum Payload {
    File(String),
    Command(Command),
}

struct Item {
    label: SharedString,
    detail: SharedString,
    payload: Payload,
}

pub struct Palette {
    kind: PaletteKind,
    input: Entity<InputState>,
    /// All workspace files (relative, `/`-separated). Shared with the shell and
    /// the background ranking task.
    files: Arc<Vec<String>>,
    /// Open files, most recently used first; shown first for an empty query.
    recent: Vec<String>,
    items: Vec<Item>,
    selected: usize,
    scroll: ScrollHandle,
    /// Bumped on every query change; results from older queries are dropped.
    generation: u64,
    /// In-flight background ranking (dropping it cancels the foreground half).
    rank_task: Option<Task<()>>,
    _subscription: Subscription,
}

impl Palette {
    pub fn new(
        kind: PaletteKind,
        files: Arc<Vec<String>>,
        recent: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match kind {
            PaletteKind::QuickOpen => "Go to file by name",
            PaletteKind::Commands => "Type a command",
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh(cx);
                }
            },
        );
        input.update(cx, |state, cx| state.focus(window, cx));

        let mut this = Self {
            kind,
            input,
            files,
            recent,
            items: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            generation: 0,
            rank_task: None,
            _subscription: subscription,
        };
        this.refresh(cx);
        this
    }

    /// Give keyboard focus back to the query input.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn kind(&self) -> PaletteKind {
        self.kind
    }

    fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_string()
    }

    /// Recompute the result list for the current query text.
    ///
    /// Commands and the empty Quick Open query are cheap and computed inline.
    /// A Quick Open query ranks every workspace file with nucleo on the
    /// background executor; a newer keystroke supersedes (and discards) the
    /// result of an older one.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.generation += 1;
        self.rank_task = None;
        match self.kind {
            PaletteKind::Commands => {
                let items = rank_commands(&query);
                self.set_items(items, cx);
            }
            PaletteKind::QuickOpen if query.is_empty() => {
                let items = self.recent_items();
                self.set_items(items, cx);
            }
            PaletteKind::QuickOpen => {
                let generation = self.generation;
                let files = self.files.clone();
                let recent = self.recent.clone();
                self.rank_task = Some(cx.spawn(async move |this, cx| {
                    let items = cx
                        .background_spawn(async move { rank_files(&files, &recent, &query) })
                        .await;
                    this.update(cx, |this, cx| {
                        if this.generation == generation {
                            this.set_items(items, cx);
                        }
                    })
                    .ok();
                }));
            }
        }
    }

    fn set_items(&mut self, items: Vec<Item>, cx: &mut Context<Self>) {
        self.items = items;
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    /// Empty query: open files (most recent first), then the first files.
    fn recent_items(&self) -> Vec<Item> {
        let mut out: Vec<Item> = self.recent.iter().map(|r| file_item(r)).collect();
        for rel in self.files.iter() {
            if out.len() >= MAX_RESULTS {
                break;
            }
            if !self.recent.contains(rel) {
                out.push(file_item(rel));
            }
        }
        out.truncate(MAX_RESULTS);
        out
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(self.selected) else {
            return;
        };
        let event = match &item.payload {
            Payload::File(rel) => PaletteEvent::Open(rel.clone()),
            Payload::Command(cmd) => PaletteEvent::Run(*cmd),
        };
        cx.emit(event);
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let m = event.keystroke.modifiers;
        if m.alt || m.platform {
            return;
        }
        let handled = match event.keystroke.key.as_str() {
            "down" => {
                self.move_selection(1, cx);
                true
            }
            "up" => {
                self.move_selection(-1, cx);
                true
            }
            "pagedown" => {
                self.move_selection(8, cx);
                true
            }
            "pageup" => {
                self.move_selection(-8, cx);
                true
            }
            "enter" => {
                self.accept(cx);
                true
            }
            "escape" => {
                cx.emit(PaletteEvent::Dismiss);
                true
            }
            _ => false,
        };
        if handled {
            // Keep the text input from also seeing these keys.
            cx.stop_propagation();
        }
    }
}

fn file_item(rel: &str) -> Item {
    let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    Item {
        label: name.to_string().into(),
        detail: dir.to_string().into(),
        payload: Payload::File(rel.to_string()),
    }
}

/// Rank `files` for `query` (top [`MAX_RESULTS`]). Runs off the UI thread.
fn rank_files(files: &[String], recent: &[String], query: &str) -> Vec<Item> {
    build_quick_open_items(files, None, query, recent, MAX_RESULTS)
        .items
        .iter()
        .map(|item| file_item(&item.path))
        .collect()
}

fn rank_commands(query: &str) -> Vec<Item> {
    let mut matcher = FuzzyMatcher::new();
    let mut scored: Vec<(i64, usize)> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(ix, c)| matcher.find(c.title, query).map(|m| (m.score, ix)))
        .collect();
    // Stable sort: equal scores (including the empty query) keep list order.
    scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
    scored
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, ix)| {
            let c = &COMMANDS[ix];
            Item {
                label: c.title.into(),
                detail: c.keys.into(),
                payload: Payload::Command(c.command),
            }
        })
        .collect()
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let selected = self.selected;
        let rows = self.items.iter().enumerate().map(|(ix, item)| {
            div()
                .id(("palette-row", ix))
                .h_flex()
                .gap_3()
                .px_3()
                .py_1()
                .items_center()
                .cursor_pointer()
                .text_sm()
                .when(ix == selected, |this| this.bg(theme.list_active))
                .when(ix != selected, |this| {
                    this.hover(|style| style.bg(theme.list_hover))
                })
                .child(div().flex_none().child(item.label.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(item.detail.clone()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = ix;
                    this.accept(cx);
                }))
        });

        // Full-window backdrop; clicking outside the panel dismisses.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)),
            )
            .child(
                div()
                    .v_flex()
                    .absolute()
                    .top(px(48.))
                    .left_0()
                    .right_0()
                    .mx_auto()
                    .w(px(560.))
                    .max_w_full()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .capture_key_down(
                        cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)),
                    )
                    .child(
                        div()
                            .p_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Input::new(&self.input)),
                    )
                    .child(
                        div()
                            .id("palette-results")
                            .v_flex()
                            .max_h(px(360.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .py_1()
                            .children(rows)
                            .when(self.items.is_empty(), |this| {
                                this.child(
                                    div()
                                        .px_3()
                                        .py_2()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child("No matching results"),
                                )
                            }),
                    ),
            )
    }
}
