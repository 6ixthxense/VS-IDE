//! File name -> language mapping for syntax highlighting and the status bar.

/// A language known to the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lang {
    /// Name understood by gpui-component's highlighter registry (empty = no grammar).
    pub id: &'static str,
    /// Human readable name for the status bar.
    pub label: &'static str,
}

const fn lang(id: &'static str, label: &'static str) -> Lang {
    Lang { id, label }
}

pub const PLAIN: Lang = lang("text", "Plain Text");

/// Detect the language from a file name (not a full path).
pub fn detect(file_name: &str) -> Lang {
    let lower = file_name.to_ascii_lowercase();
    match lower.as_str() {
        "makefile" | "gnumakefile" => return lang("text", "Makefile"),
        "dockerfile" => return lang("bash", "Dockerfile"),
        "cmakelists.txt" => return lang("text", "CMake"),
        ".bashrc" | ".zshrc" | ".profile" => return lang("bash", "Shell"),
        _ => {}
    }
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "ts" | "mts" | "cts" => lang("typescript", "TypeScript"),
        "tsx" => lang("tsx", "TypeScript React"),
        "js" | "mjs" | "cjs" | "jsx" => lang("javascript", "JavaScript"),
        "json" | "jsonc" | "json5" | "webmanifest" => lang("json", "JSON"),
        "py" | "pyi" | "pyw" => lang("python", "Python"),
        "rs" => lang("rust", "Rust"),
        "md" | "markdown" | "mdx" => lang("markdown", "Markdown"),
        "toml" => lang("toml", "TOML"),
        "yaml" | "yml" => lang("yaml", "YAML"),
        "html" | "htm" | "xml" | "svg" | "vue" => lang("html", "HTML"),
        "css" | "scss" | "less" => lang("css", "CSS"),
        "sh" | "bash" | "zsh" => lang("bash", "Shell"),
        "go" => lang("go", "Go"),
        "c" | "h" => lang("c", "C"),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => lang("cpp", "C++"),
        "java" => lang("java", "Java"),
        "sql" => lang("sql", "SQL"),
        "txt" | "log" | "" => PLAIN,
        _ => PLAIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_languages() {
        assert_eq!(detect("main.rs").id, "rust");
        assert_eq!(detect("App.TSX").id, "tsx");
        assert_eq!(detect("a.test.ts").id, "typescript");
        assert_eq!(detect("package.json").label, "JSON");
        assert_eq!(detect("README").id, "text");
        assert_eq!(detect("Makefile").label, "Makefile");
    }
}
