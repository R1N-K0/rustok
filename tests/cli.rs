//! End-to-end tests driving the built binary.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;

fn tok() -> Command {
    let mut cmd = Command::cargo_bin("tok").unwrap();
    // Keep output deterministic regardless of the developer's environment.
    cmd.env_remove("TOK_MODEL").arg("--color").arg("never");
    cmd
}

/// A small tree with a gitignored file, a hidden file and a binary file.
fn fixture(name: &str) -> tempdir::TempDir {
    let dir = tempdir::TempDir::new(name);
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/b.md"), "# hello world\n").unwrap();
    fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(root.join("ignored.txt"), "this should not be counted\n").unwrap();
    fs::write(root.join(".hidden.txt"), "hidden text\n").unwrap();
    fs::write(root.join("blob.bin"), [0x00, 0x01, 0x02, 0x00]).unwrap();
    dir
}

fn quiet_count(dir: &Path, extra: &[&str]) -> usize {
    let out = tok().arg(dir).args(extra).arg("--quiet").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn counts_text_from_stdin() {
    tok()
        .write_stdin("hello world")
        .assert()
        .success()
        .stdout(contains("2 tokens"));
}

#[test]
fn quiet_prints_only_the_number() {
    let out = tok()
        .arg("--quiet")
        .write_stdin("hello world")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2");
}

#[test]
fn model_and_encoding_agree_for_the_same_family() {
    let by_model = tok()
        .args(["-m", "gpt-4o", "-q", "-t", "hello world"])
        .output()
        .unwrap();
    let by_encoding = tok()
        .args(["-e", "o200k_base", "-q", "-t", "hello world"])
        .output()
        .unwrap();
    assert_eq!(by_model.stdout, by_encoding.stdout);
}

#[test]
fn different_encodings_give_different_counts() {
    let o200k = tok()
        .args(["-e", "o200k_base", "-q", "-t", "こんにちは世界"])
        .output()
        .unwrap();
    let r50k = tok()
        .args(["-e", "r50k_base", "-q", "-t", "こんにちは世界"])
        .output()
        .unwrap();
    assert_ne!(o200k.stdout, r50k.stdout);
}

#[test]
fn model_and_encoding_cannot_be_combined() {
    tok()
        .args(["-m", "gpt-4o", "-e", "cl100k_base", "-t", "x"])
        .assert()
        .failure()
        .stderr(contains("cannot be used with"));
}

#[test]
fn unknown_model_exits_with_status_two() {
    let out = tok().args(["-m", "llama-3", "-t", "x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown model"));
}

#[test]
fn walking_a_directory_respects_gitignore_and_skips_binaries() {
    let dir = fixture("walk");
    let out = tok().arg(dir.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("a.rs"), "{stdout}");
    assert!(stdout.contains("b.md"), "{stdout}");
    assert!(!stdout.contains("ignored.txt"), "{stdout}");
    assert!(!stdout.contains(".hidden.txt"), "{stdout}");
    assert!(!stdout.contains("blob.bin"), "{stdout}");
    assert!(stdout.contains("2 files"), "{stdout}");
}

#[test]
fn no_ignore_and_hidden_widen_the_walk() {
    let dir = fixture("widen");
    let base = quiet_count(dir.path(), &[]);
    let wider = quiet_count(dir.path(), &["--no-ignore", "--hidden"]);
    assert!(wider > base, "expected {wider} > {base}");
}

#[test]
fn ext_filter_selects_a_single_language() {
    let dir = fixture("ext");
    let all = quiet_count(dir.path(), &[]);
    let rust_only = quiet_count(dir.path(), &["--ext", "rs"]);
    assert!(rust_only > 0);
    assert!(rust_only < all, "expected {rust_only} < {all}");
}

#[test]
fn exclude_glob_drops_matching_paths() {
    let dir = fixture("exclude");
    let all = quiet_count(dir.path(), &[]);
    let without_md = quiet_count(dir.path(), &["--exclude", "*.md"]);
    assert!(without_md < all, "expected {without_md} < {all}");
}

#[test]
fn an_explicitly_named_file_is_counted_even_when_gitignored() {
    let dir = fixture("explicit");
    let out = tok()
        .arg(dir.path().join("ignored.txt"))
        .arg("--quiet")
        .output()
        .unwrap();
    assert!(out.status.success());
    let count: usize = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    assert!(count > 0);
}

#[test]
fn limit_exits_one_only_when_exceeded() {
    let dir = fixture("limit");
    let over = tok()
        .arg(dir.path())
        .args(["--limit", "1"])
        .output()
        .unwrap();
    assert_eq!(over.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&over.stderr).contains("over limit"));

    let under = tok()
        .arg(dir.path())
        .args(["--limit", "1000000"])
        .output()
        .unwrap();
    assert_eq!(under.status.code(), Some(0));
}

#[test]
fn json_output_is_valid_and_totals_match() {
    let dir = fixture("json");
    let out = tok()
        .arg(dir.path())
        .args(["--format", "json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    assert_eq!(json["model"], "gpt-5");
    assert_eq!(json["encoding"], "o200k_base");
    assert_eq!(json["context_window"], 400_000);

    let files = json["files"].as_array().unwrap();
    let summed: u64 = files.iter().map(|f| f["tokens"].as_u64().unwrap()).sum();
    assert_eq!(summed, json["total_tokens"].as_u64().unwrap());
    assert_eq!(json["skipped_binary"].as_array().unwrap().len(), 1);
}

#[test]
fn csv_output_has_a_header_and_a_total_row() {
    let dir = fixture("csv");
    let out = tok()
        .arg(dir.path())
        .args(["--format", "csv"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();

    assert_eq!(lines.next().unwrap(), "file,tokens,bytes,lines");
    assert!(stdout.lines().last().unwrap().starts_with("TOTAL,"));
}

#[test]
fn by_ext_groups_rows_and_keeps_the_total() {
    let dir = fixture("byext");
    let plain = quiet_count(dir.path(), &[]);
    let out = tok()
        .arg(dir.path())
        .args(["--by-ext", "--format", "csv"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.starts_with("ext,tokens"), "{stdout}");
    assert!(stdout.contains(".rs,"), "{stdout}");
    assert!(stdout.contains(".md,"), "{stdout}");
    assert!(stdout.contains(&format!("TOTAL,{plain},,")), "{stdout}");
}

#[test]
fn top_limits_rows_without_changing_the_total() {
    let dir = fixture("top");
    let all = quiet_count(dir.path(), &[]);
    let out = tok().arg(dir.path()).args(["-n", "1"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("and 1 more"), "{stdout}");
    assert!(stdout.contains(&all.to_string()), "{stdout}");
}

#[test]
fn ids_round_trip_through_decode() {
    let ids = tok()
        .args(["--ids", "-t", "日本語のトークン"])
        .output()
        .unwrap();
    assert!(ids.status.success());

    let decoded = tok()
        .arg("decode")
        .write_stdin(String::from_utf8_lossy(&ids.stdout).into_owned())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&decoded.stdout), "日本語のトークン");
}

#[test]
fn decode_rejects_ids_that_are_not_numbers() {
    let out = tok().args(["decode", "not-a-token"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn show_reproduces_the_input_text() {
    let out = tok()
        .args(["--show", "-t", "こんにちは 🎉 world"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("こんにちは 🎉 world"), "{stdout}");
    assert!(!stdout.contains('\u{fffd}'), "{stdout}");
}

#[test]
fn special_tokens_are_literal_unless_asked_for() {
    let literal = tok().args(["-q", "-t", "<|endoftext|>"]).output().unwrap();
    let special = tok()
        .args(["-q", "--special", "-t", "<|endoftext|>"])
        .output()
        .unwrap();

    assert_eq!(String::from_utf8_lossy(&special.stdout).trim(), "1");
    assert!(String::from_utf8_lossy(&literal.stdout).trim() != "1");
}

#[test]
fn thread_count_does_not_change_results() {
    let dir = fixture("threads");
    assert_eq!(
        quiet_count(dir.path(), &["-j", "1"]),
        quiet_count(dir.path(), &["-j", "4"])
    );
}

#[test]
fn models_lists_known_models() {
    tok()
        .arg("models")
        .assert()
        .success()
        .stdout(contains("gpt-5").and(contains("o200k_base")));
}

#[test]
fn completions_generate_for_each_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let out = tok().args(["completions", shell]).output().unwrap();
        assert!(out.status.success(), "{shell} completions failed");
        assert!(!out.stdout.is_empty(), "{shell} completions were empty");
    }
}

#[test]
fn a_missing_path_is_an_error_not_a_zero_count() {
    let out = tok().arg("definitely/not/here.txt").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

/// Minimal scratch directory helper, so the test suite needs no extra crate.
mod tempdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(name: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("tok-test-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
