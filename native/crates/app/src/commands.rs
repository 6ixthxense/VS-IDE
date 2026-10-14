//! Actions (bound to keys) and the command-palette command list.

use gpui::actions;

actions!(
    vsm,
    [
        Save,
        CloseTab,
        NextTab,
        PrevTab,
        QuickOpen,
        CommandPalette,
        ToggleSidebar,
        Quit,
    ]
);

/// Everything the palette can run. Backend crates add variants here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Save,
    CloseTab,
    NextTab,
    PrevTab,
    QuickOpen,
    ToggleSidebar,
    RefreshExplorer,
    /// Placeholder for features owned by other crates (terminal, git, ...).
    Stub(&'static str),
}

pub struct CommandInfo {
    pub title: &'static str,
    pub keys: &'static str,
    pub command: Command,
}

const fn info(title: &'static str, keys: &'static str, command: Command) -> CommandInfo {
    CommandInfo { title, keys, command }
}

pub const COMMANDS: &[CommandInfo] = &[
    info("File: Save", "Ctrl+S", Command::Save),
    info("File: Close Editor", "Ctrl+W", Command::CloseTab),
    info("File: Quick Open...", "Ctrl+P", Command::QuickOpen),
    info("View: Toggle Sidebar", "Ctrl+B", Command::ToggleSidebar),
    info("View: Next Editor", "Ctrl+PageDown", Command::NextTab),
    info("View: Previous Editor", "Ctrl+PageUp", Command::PrevTab),
    info("Explorer: Refresh", "", Command::RefreshExplorer),
    info("Terminal: Toggle Panel", "", Command::Stub("Terminal panel (vsm-terminal)")),
    info("Search: Find in Files", "", Command::Stub("Search (vsm-search)")),
    info("Git: Show Source Control", "", Command::Stub("Source control (vsm-git)")),
    info("System: Show Monitor", "", Command::Stub("System monitor (vsm-sysmon)")),
    info("Database: Open Connection", "", Command::Stub("Database (vsm-db)")),
    info("Problems: Show Panel", "", Command::Stub("Problems (vsm-tasks / vsm-lsp)")),
];
