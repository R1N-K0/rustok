//! Reading and counting the collected inputs, in parallel.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::encoding::Encoder;
use crate::input::{Item, display_path};

/// Bytes inspected when deciding whether a file is binary.
const SNIFF_LEN: usize = 8192;

#[derive(Debug, Clone)]
pub struct FileStat {
    pub label: String,
    pub tokens: usize,
    pub bytes: u64,
    pub lines: usize,
}

/// What happened to one input.
enum Outcome {
    Counted(FileStat),
    SkippedBinary(PathBuf),
    Failed(PathBuf, String),
}

/// Everything the output layer needs.
pub struct Report {
    pub stats: Vec<FileStat>,
    pub skipped_binary: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

impl Report {
    pub fn total_tokens(&self) -> usize {
        self.stats.iter().map(|s| s.tokens).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.stats.iter().map(|s| s.bytes).sum()
    }
}

/// Count every item, reading files on a rayon pool.
///
/// `threads` of 0 means "let rayon decide".
pub fn count_all(items: Vec<Item>, enc: &Encoder, threads: usize) -> Result<Report> {
    let outcomes: Vec<Outcome> = if threads == 1 {
        items.into_iter().map(|item| count_one(item, enc)).collect()
    } else {
        let run = || {
            items
                .into_par_iter()
                .map(|item| count_one(item, enc))
                .collect()
        };
        if threads == 0 {
            run()
        } else {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .context("failed to start the thread pool")?
                .install(run)
        }
    };

    let mut report = Report {
        stats: Vec::new(),
        skipped_binary: Vec::new(),
        failed: Vec::new(),
    };
    for outcome in outcomes {
        match outcome {
            Outcome::Counted(stat) => report.stats.push(stat),
            Outcome::SkippedBinary(path) => report.skipped_binary.push(path),
            Outcome::Failed(path, err) => report.failed.push((path, err)),
        }
    }
    Ok(report)
}

fn count_one(item: Item, enc: &Encoder) -> Outcome {
    match item {
        Item::Inline { label, text } => Outcome::Counted(stat_from_text(label, &text, enc)),
        Item::File(path) => match std::fs::read(&path) {
            Err(err) => Outcome::Failed(path, err.to_string()),
            Ok(bytes) if is_binary(&bytes) => Outcome::SkippedBinary(path),
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                Outcome::Counted(stat_from_text(display_path(&path), &text, enc))
            }
        },
    }
}

fn stat_from_text(label: String, text: &str, enc: &Encoder) -> FileStat {
    FileStat {
        label,
        tokens: enc.count(text),
        bytes: text.len() as u64,
        lines: text.lines().count(),
    }
}

/// A NUL byte near the start is the same heuristic git and ripgrep use.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(SNIFF_LEN).any(|&b| b == 0)
}

/// Fold per-file stats into per-extension stats. Files with no extension are
/// grouped under their file name, so `Makefile` and `Dockerfile` stay readable.
pub fn by_extension(stats: &[FileStat]) -> Vec<FileStat> {
    use std::collections::HashMap;

    let mut groups: HashMap<String, FileStat> = HashMap::new();
    for stat in stats {
        let key = extension_key(&stat.label);
        let entry = groups.entry(key.clone()).or_insert_with(|| FileStat {
            label: key,
            tokens: 0,
            bytes: 0,
            lines: 0,
        });
        entry.tokens += stat.tokens;
        entry.bytes += stat.bytes;
        entry.lines += stat.lines;
    }
    groups.into_values().collect()
}

fn extension_key(label: &str) -> String {
    let name = label.rsplit('/').next().unwrap_or(label);
    match name.rsplit_once('.') {
        // A leading dot means a dotfile like `.gitignore`, not an extension.
        Some((stem, ext)) if !stem.is_empty() => format!(".{}", ext.to_ascii_lowercase()),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(label: &str, tokens: usize) -> FileStat {
        FileStat {
            label: label.to_string(),
            tokens,
            bytes: tokens as u64 * 4,
            lines: 1,
        }
    }

    #[test]
    fn nul_byte_marks_a_file_binary() {
        assert!(is_binary(b"\x7fELF\0\0\0"));
        assert!(!is_binary("hello world".as_bytes()));
        assert!(!is_binary("日本語のテキスト".as_bytes()));
    }

    #[test]
    fn nul_bytes_past_the_sniff_window_are_ignored() {
        let mut bytes = vec![b'a'; SNIFF_LEN];
        bytes.push(0);
        assert!(!is_binary(&bytes));
    }

    #[test]
    fn extension_keys_handle_dotfiles_and_bare_names() {
        assert_eq!(extension_key("src/main.rs"), ".rs");
        assert_eq!(extension_key("README.MD"), ".md");
        assert_eq!(extension_key(".gitignore"), ".gitignore");
        assert_eq!(extension_key("Makefile"), "Makefile");
        assert_eq!(extension_key("a/b/c.tar.gz"), ".gz");
    }

    #[test]
    fn grouping_sums_tokens_per_extension() {
        let stats = vec![stat("a.rs", 10), stat("b/c.rs", 5), stat("d.md", 7)];
        let mut grouped = by_extension(&stats);
        grouped.sort_by(|a, b| a.label.cmp(&b.label));

        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].label, ".md");
        assert_eq!(grouped[0].tokens, 7);
        assert_eq!(grouped[1].label, ".rs");
        assert_eq!(grouped[1].tokens, 15);
    }
}
