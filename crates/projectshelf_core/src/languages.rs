use crate::config::IGNORED_DIRS;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct LanguageBreakdown {
    pub languages: Vec<(String, u64)>,
    pub total_bytes: u64,
}

/// Languages excluded from being reported as a project's *primary* language —
/// data/config formats that often dominate by bytes (e.g. a giant
/// `package-lock.json`) but don't characterize the codebase. They still appear
/// in the full breakdown / `top_n`.
const NON_PRIMARY_LANGUAGES: &[&str] = &["JSON"];

impl LanguageBreakdown {
    /// The top language by bytes, skipping non-code data formats
    /// ([`NON_PRIMARY_LANGUAGES`]). `None` if there's no real code language.
    pub fn primary_language(&self) -> Option<&str> {
        self.languages
            .iter()
            .map(|(lang, _)| lang.as_str())
            .find(|lang| !NON_PRIMARY_LANGUAGES.contains(lang))
    }

    pub fn top_n(&self, n: usize) -> Vec<(&str, u64, f32)> {
        self.languages
            .iter()
            .take(n)
            .map(|(lang, bytes)| {
                let pct = if self.total_bytes > 0 {
                    (*bytes as f32 / self.total_bytes as f32) * 100.0
                } else {
                    0.0
                };
                (lang.as_str(), *bytes, pct)
            })
            .collect()
    }
}

pub fn detect_languages(path: &Path) -> LanguageBreakdown {
    let mut lang_bytes: HashMap<String, u64> = HashMap::new();
    scan_for_languages(path, 0, &mut lang_bytes);

    let mut languages: Vec<_> = lang_bytes.into_iter().collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1));

    let total_bytes = languages.iter().map(|(_, b)| *b).sum();

    LanguageBreakdown {
        languages,
        total_bytes,
    }
}

fn scan_for_languages(path: &Path, depth: usize, lang_bytes: &mut HashMap<String, u64>) {
    if depth > 5 {
        return;
    }

    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if IGNORED_DIRS.contains(&dir_name) {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if IGNORED_DIRS.contains(&name) {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                if let Some(lang) = extension_to_language(&entry_path) {
                    *lang_bytes.entry(lang.to_string()).or_insert(0) += metadata.len();
                }
            } else if metadata.is_dir() {
                scan_for_languages(&entry_path, depth + 1, lang_bytes);
            }
        }
    }
}

fn extension_to_language(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    let ext_str = ext.as_str();

    match ext_str {
        "rs" => Some("Rust"),
        "py" | "pyw" | "pyi" => Some("Python"),
        "js" | "mjs" | "cjs" => Some("JavaScript"),
        "ts" | "mts" | "cts" => Some("TypeScript"),
        "tsx" => Some("TSX"),
        "jsx" => Some("JSX"),
        "go" => Some("Go"),
        "c" | "h" => Some("C"),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some("C++"),
        "java" => Some("Java"),
        "kt" | "kts" => Some("Kotlin"),
        "swift" => Some("Swift"),
        "rb" | "erb" => Some("Ruby"),
        "php" => Some("PHP"),
        "cs" => Some("C#"),
        "fs" | "fsx" | "fsi" => Some("F#"),
        "scala" | "sc" => Some("Scala"),
        "clj" | "cljs" | "cljc" | "edn" => Some("Clojure"),
        "ex" | "exs" => Some("Elixir"),
        "erl" | "hrl" => Some("Erlang"),
        "hs" | "lhs" => Some("Haskell"),
        "ml" | "mli" => Some("OCaml"),
        "lua" => Some("Lua"),
        "pl" | "pm" => Some("Perl"),
        "r" => Some("R"),
        "jl" => Some("Julia"),
        "dart" => Some("Dart"),
        "zig" => Some("Zig"),
        "nim" => Some("Nim"),
        "v" => Some("V"),
        "d" => Some("D"),
        "cr" => Some("Crystal"),
        "sh" | "bash" | "zsh" | "fish" => Some("Shell"),
        "ps1" | "psm1" | "psd1" => Some("PowerShell"),
        "sql" => Some("SQL"),
        "html" | "htm" => Some("HTML"),
        "css" => Some("CSS"),
        "scss" | "sass" => Some("SCSS"),
        "less" => Some("Less"),
        "vue" => Some("Vue"),
        "svelte" => Some("Svelte"),
        "json" => Some("JSON"),
        "yaml" | "yml" => Some("YAML"),
        "toml" => Some("TOML"),
        "xml" => Some("XML"),
        "md" | "markdown" => Some("Markdown"),
        "rst" => Some("reStructuredText"),
        "tex" | "latex" => Some("LaTeX"),
        "dockerfile" => Some("Dockerfile"),
        "makefile" => Some("Makefile"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breakdown(items: &[(&str, u64)]) -> LanguageBreakdown {
        let languages: Vec<(String, u64)> =
            items.iter().map(|(l, b)| (l.to_string(), *b)).collect();
        let total_bytes = languages.iter().map(|(_, b)| *b).sum();
        LanguageBreakdown { languages, total_bytes }
    }

    #[test]
    fn primary_language_skips_json() {
        // JSON dominates by bytes but Rust should be reported as primary.
        let b = breakdown(&[("JSON", 1_000), ("Rust", 500)]);
        assert_eq!(b.primary_language(), Some("Rust"));
    }

    #[test]
    fn primary_language_none_when_only_json() {
        let b = breakdown(&[("JSON", 1_000)]);
        assert_eq!(b.primary_language(), None);
    }

    #[test]
    fn primary_language_normal_case() {
        let b = breakdown(&[("Rust", 1_000), ("TOML", 100)]);
        assert_eq!(b.primary_language(), Some("Rust"));
    }
}
