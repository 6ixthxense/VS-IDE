//! Bottom status bar.
//!
//! A separate view so cursor movement re-renders only this 22px strip, not the
//! whole shell. It observes the active editor entity and only calls
//! `cx.notify()` when the displayed line/column actually changed.

use gpui::{div, prelude::*, px, Context, Entity, SharedString, Subscription, Window};
use gpui_component::{input::EditorState, ActiveTheme as _, StyledExt as _};

#[derive(Clone, PartialEq, Eq)]
pub struct DocInfo {
    pub path: SharedString,
    pub language: SharedString,
    pub dirty: bool,
    pub crlf: bool,
}

pub struct StatusBar {
    workspace: SharedString,
    doc: Option<DocInfo>,
    notice: Option<SharedString>,
    /// 1-based (line, column) of the cursor in the active editor.
    pos: (u32, u32),
    observed: Option<Subscription>,
}

impl StatusBar {
    pub fn new(workspace: impl Into<SharedString>) -> Self {
        Self {
            workspace: workspace.into(),
            doc: None,
            notice: None,
            pos: (1, 1),
            observed: None,
        }
    }

    /// Point the bar at `editor` (or at nothing).
    pub fn set_document(
        &mut self,
        doc: Option<(DocInfo, Entity<EditorState>)>,
        cx: &mut Context<Self>,
    ) {
        match doc {
            Some((info, editor)) => {
                self.doc = Some(info);
                self.pos = cursor(&editor, cx);
                self.observed = Some(cx.observe(&editor, |this, editor, cx| {
                    let pos = cursor(&editor, cx);
                    if pos != this.pos {
                        this.pos = pos;
                        cx.notify();
                    }
                }));
            }
            None => {
                self.doc = None;
                self.observed = None;
                self.pos = (1, 1);
            }
        }
        cx.notify();
    }

    pub fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        if let Some(doc) = &mut self.doc {
            if doc.dirty != dirty {
                doc.dirty = dirty;
                cx.notify();
            }
        }
    }

    pub fn set_notice(&mut self, notice: Option<SharedString>, cx: &mut Context<Self>) {
        if self.notice != notice {
            self.notice = notice;
            cx.notify();
        }
    }
}

fn cursor(editor: &Entity<EditorState>, cx: &gpui::App) -> (u32, u32) {
    let p = editor.read(cx).cursor_position();
    (p.line + 1, p.character + 1)
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let fg = theme.primary_foreground;
        let dim = fg.opacity(0.8);

        let left = div()
            .h_flex()
            .gap_3()
            .min_w_0()
            .child(div().flex_none().child(self.workspace.clone()))
            .when_some(self.notice.clone(), |this, notice| {
                this.child(div().truncate().child(notice))
            });

        let right = div().h_flex().gap_4().min_w_0().justify_end().when_some(
            self.doc.clone(),
            |this, doc| {
                this.child(
                    div()
                        .h_flex()
                        .gap_1()
                        .min_w_0()
                        .child(div().truncate().child(doc.path))
                        .when(doc.dirty, |this| {
                            this.child(
                                div()
                                    .id("dirty-marker")
                                    .flex_none()
                                    .text_color(theme.warning)
                                    .child("\u{25CF}"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .child(format!("Ln {}, Col {}", self.pos.0, self.pos.1)),
                )
                .child(div().flex_none().text_color(dim).child(if doc.crlf {
                    "CRLF"
                } else {
                    "LF"
                }))
                .child(div().flex_none().child(doc.language))
            },
        );

        div()
            .h_flex()
            .flex_none()
            .h(px(22.))
            .w_full()
            .px_3()
            .gap_4()
            .justify_between()
            .overflow_hidden()
            .text_xs()
            .bg(theme.status_bar)
            .text_color(fg)
            .child(left)
            .child(right)
    }
}
