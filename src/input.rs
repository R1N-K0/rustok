//! Turning CLI paths into a flat list of things to count.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

use crate::cli::WalkArgs;

/// One unit of input. Files are read later so that the reads can be parallelised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A path on disk, read during counting.
    File(PathBuf),
    /// Text already in memory: stdin or `--text`.
    Inline { label: String, text: String },
}

/// Render a path for output: forward slashes everywhere, and strip the `./` noise
/// that `ignore` adds when walking the current directory.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("./").unwrap_or(&text).to_string()
}

/// Collect the inputs named on the command line.
///
/// With no paths and no `--text`, stdin is read. A path of `-` also means stdin.
pub fn collect(paths: &[PathBuf], text: Option<&str>, walk: &WalkArgs) -> Result<Vec<Item>> {
    if let Some(text) = text {
        return Ok(vec![Item::Inline {
            label: "<text>".to_string(),
            text: text.to_string(),
        }]);
    }

    if paths.is_empty() {
        return Ok(vec![read_stdin()?]);
    }

    let mut items = Vec::new();
    let mut seen_stdin = false;

    for path in paths {
        if path.as_os_str() == "-" {
            if !seen_stdin {
                items.push(read_stdin()?);
                seen_stdin = true;
            }
            continue;
        }

        let meta = std::fs::metadata(path)
            .with_context(|| format!("cannot read `{}`", display_path(path)))?;

        if meta.is_dir() {
            items.extend(walk_dir(path, walk)?);
        } else {
            // An explicitly named file is counted even if .gitignore would hide it.
            items.push(Item::File(path.clone()));
        }
    }

    if items.is_empty() {
        bail!("no files to count (try --hidden, --no-ignore, or a wider --glob)");
    }

    Ok(items)
}

fn read_stdin() -> Result<Item> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .context("failed to read stdin")?;
    Ok(Item::Inline {
        label: "<stdin>".to_string(),
        text: String::from_utf8_lossy(&buf).into_owned(),
    })
}

fn walk_dir(root: &Path, args: &WalkArgs) -> Result<Vec<Item>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!args.hidden)
        .git_ignore(!args.no_ignore)
        .git_global(!args.no_ignore)
        .git_exclude(!args.no_ignore)
        .ignore(!args.no_ignore)
        .parents(!args.no_ignore)
        // Honour .gitignore even outside a git repository: an unpacked tarball
        // or a scratch directory should filter the same way the repo does.
        .require_git(false)
        .follow_links(args.follow);

    if !args.glob.is_empty() || !args.exclude.is_empty() {
        let mut overrides = OverrideBuilder::new(root);
        for glob in &args.glob {
            overrides
                .add(glob)
                .with_context(|| format!("invalid --glob pattern `{glob}`"))?;
        }
        for glob in &args.exclude {
            overrides
                .add(&format!("!{glob}"))
                .with_context(|| format!("invalid --exclude pattern `{glob}`"))?;
        }
        builder.overrides(overrides.build()?);
    }

    let exts: Vec<String> = args.ext.iter().map(|e| normalise_ext(e)).collect();

    let mut items = Vec::new();
    for entry in builder.build() {
        let entry = entry.context("failed to walk directory")?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if !exts.is_empty() && !has_ext(entry.path(), &exts) {
            continue;
        }
        items.push(Item::File(entry.into_path()));
    }

    Ok(items)
}

/// Accept `--ext rs`, `--ext .rs` and `--ext RS` alike.
fn normalise_ext(ext: &str) -> String {
    ext.trim_start_matches('.').to_ascii_lowercase()
}

fn has_ext(path: &Path, exts: &[String]) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| exts.contains(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_matching_ignores_case_and_leading_dot() {
        let exts = vec![normalise_ext(".RS"), normalise_ext("md")];
        assert_eq!(exts, vec!["rs", "md"]);
        assert!(has_ext(Path::new("src/main.rs"), &exts));
        assert!(has_ext(Path::new("README.MD"), &exts));
        assert!(!has_ext(Path::new("Cargo.toml"), &exts));
        assert!(!has_ext(Path::new("Makefile"), &exts));
    }

    #[test]
    fn display_path_normalises_separators() {
        assert_eq!(display_path(Path::new("./src/main.rs")), "src/main.rs");
        assert_eq!(display_path(Path::new(r"src\main.rs")), "src/main.rs");
    }

    #[test]
    fn text_wins_over_stdin() {
        let items = collect(&[], Some("hello"), &WalkArgs::default()).unwrap();
        assert_eq!(
            items,
            vec![Item::Inline {
                label: "<text>".into(),
                text: "hello".into()
            }]
        );
    }
}
