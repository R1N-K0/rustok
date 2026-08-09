//! Rendering reports as a table, JSON, CSV or a bare number.

use anstream::{eprintln, println};
use anstyle::{AnsiColor, Style};
use anyhow::Result;
use serde::Serialize;

use crate::cli::{CountArgs, Format, Sort};
use crate::count::{FileStat, Report, by_extension};
use crate::encoding::{Encoder, model_table};

const BAR_WIDTH: usize = 12;

const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();
const BAR: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));
const WARN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));

/// Group `12345` as `12,345`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Byte counts as `1.4 KiB`, keeping whole bytes exact.
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Order and trim the rows according to `--by-ext`, `--sort`, `--reverse` and `--top`.
fn prepare_rows(report: &Report, args: &CountArgs) -> (Vec<FileStat>, usize, usize) {
    let mut rows = if args.by_ext {
        by_extension(&report.stats)
    } else {
        report.stats.clone()
    };

    match args.sort {
        Sort::Tokens => rows.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.label.cmp(&b.label))),
        Sort::Size => rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.label.cmp(&b.label))),
        Sort::Path => rows.sort_by(|a, b| a.label.cmp(&b.label)),
        Sort::None => {}
    }
    if args.reverse {
        rows.reverse();
    }

    let shown = args.top.unwrap_or(rows.len()).min(rows.len());
    let hidden_tokens = rows[shown..].iter().map(|r| r.tokens).sum();
    let hidden_count = rows.len() - shown;
    rows.truncate(shown);
    (rows, hidden_count, hidden_tokens)
}

pub fn render(report: &Report, enc: &Encoder, args: &CountArgs) -> Result<()> {
    warn_about_problems(report);

    let total = report.total_tokens();
    if args.quiet {
        println!("{total}");
        return Ok(());
    }

    let (rows, hidden_count, hidden_tokens) = prepare_rows(report, args);
    let heading = if args.by_ext { "EXT" } else { "FILE" };

    match args.format {
        Format::Json => render_json(report, enc, &rows)?,
        Format::Csv => render_delimited(&rows, total, ',', heading),
        Format::Tsv => render_delimited(&rows, total, '\t', heading),
        Format::Table => render_table(report, enc, &rows, hidden_count, hidden_tokens, heading),
    }
    Ok(())
}

fn render_table(
    report: &Report,
    enc: &Encoder,
    rows: &[FileStat],
    hidden_count: usize,
    hidden_tokens: usize,
    heading: &str,
) {
    let total = report.total_tokens();

    // A single input needs no table: one summary line says everything.
    if report.stats.len() <= 1 {
        println!("{}", summary_line(total, enc));
        return;
    }

    let width = rows
        .iter()
        .map(|r| thousands(r.tokens as u64).len())
        .chain(std::iter::once(thousands(total as u64).len()))
        .max()
        .unwrap_or(1);
    let max = rows.iter().map(|r| r.tokens).max().unwrap_or(0);

    println!(
        "{DIM}{:>width$}  {:<BAR_WIDTH$}  {heading}{DIM:#}",
        "TOKENS", ""
    );
    for row in rows {
        println!(
            "{:>width$}  {BAR}{}{BAR:#}  {}",
            thousands(row.tokens as u64),
            bar(row.tokens, max),
            row.label
        );
    }
    if hidden_count > 0 {
        println!(
            "{DIM}{:>width$}  {:<BAR_WIDTH$}  … and {hidden_count} more{DIM:#}",
            thousands(hidden_tokens as u64),
            ""
        );
    }

    println!("{DIM}{}{DIM:#}", "─".repeat(width + BAR_WIDTH + 4 + 24));
    println!(
        "{BOLD}{:>width$}{BOLD:#}  {DIM}tokens in {} · {}{DIM:#}",
        thousands(total as u64),
        plural(report.stats.len(), "file"),
        human_bytes(report.total_bytes()),
    );
    println!("{:>width$}  {DIM}{}{DIM:#}", "", context_line(total, enc));
}

/// `1,234 tokens · gpt-5 (o200k_base) · 0.3% of 400,000 context`
fn summary_line(total: usize, enc: &Encoder) -> String {
    format!(
        "{BOLD}{}{BOLD:#} tokens {DIM}·{DIM:#} {}",
        thousands(total as u64),
        context_line(total, enc)
    )
}

fn context_line(total: usize, enc: &Encoder) -> String {
    let Some(window) = enc.context_window() else {
        return enc.label();
    };
    let ratio = total as f64 / window as f64;
    // Past the window a percentage stops being readable ("19055.5%"), so switch
    // to "how many times over" instead.
    let fit = if ratio > 1.0 {
        format!("{ratio:.1}× the {} token context", thousands(window as u64))
    } else if ratio >= 0.0005 {
        format!(
            "{:.1}% of {} context",
            ratio * 100.0,
            thousands(window as u64)
        )
    } else {
        // Too small to register as a fraction of the window: "0.0% of 400,000
        // context" is noise on a one-line input.
        return enc.label();
    };
    format!("{} · {fit}", enc.label())
}

fn bar(value: usize, max: usize) -> String {
    if max == 0 {
        return " ".repeat(BAR_WIDTH);
    }
    // Anything non-zero gets at least one block so it stays visible.
    let filled = ((value as f64 / max as f64) * BAR_WIDTH as f64).round() as usize;
    let filled = filled.clamp(usize::from(value > 0), BAR_WIDTH);
    format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled))
}

fn plural(n: usize, word: &str) -> String {
    let count = thousands(n as u64);
    if n == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    encoding: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window: Option<usize>,
    total_tokens: usize,
    total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_used: Option<f64>,
    files: Vec<JsonEntry<'a>>,
    skipped_binary: Vec<String>,
    failed: Vec<JsonFailure<'a>>,
}

#[derive(Serialize)]
struct JsonEntry<'a> {
    path: &'a str,
    tokens: usize,
    bytes: u64,
    lines: usize,
}

#[derive(Serialize)]
struct JsonFailure<'a> {
    path: String,
    error: &'a str,
}

fn render_json(report: &Report, enc: &Encoder, rows: &[FileStat]) -> Result<()> {
    let total = report.total_tokens();
    let json = JsonReport {
        model: enc.model(),
        encoding: enc.encoding(),
        context_window: enc.context_window(),
        total_tokens: total,
        total_bytes: report.total_bytes(),
        context_used: enc
            .context_window()
            .map(|w| (total as f64 / w as f64 * 1000.0).round() / 1000.0),
        files: rows
            .iter()
            .map(|r| JsonEntry {
                path: &r.label,
                tokens: r.tokens,
                bytes: r.bytes,
                lines: r.lines,
            })
            .collect(),
        skipped_binary: report
            .skipped_binary
            .iter()
            .map(|p| crate::input::display_path(p))
            .collect(),
        failed: report
            .failed
            .iter()
            .map(|(path, error)| JsonFailure {
                path: crate::input::display_path(path),
                error,
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn render_delimited(rows: &[FileStat], total: usize, sep: char, heading: &str) {
    println!("{}{sep}tokens{sep}bytes{sep}lines", heading.to_lowercase());
    for row in rows {
        println!(
            "{}{sep}{}{sep}{}{sep}{}",
            escape(&row.label, sep),
            row.tokens,
            row.bytes,
            row.lines
        );
    }
    println!("TOTAL{sep}{total}{sep}{sep}");
}

/// Quote a CSV field only when it would otherwise break the row.
fn escape(field: &str, sep: char) -> String {
    if field.contains(sep) || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn warn_about_problems(report: &Report) {
    for (path, error) in &report.failed {
        eprintln!(
            "{WARN}warning{WARN:#}: skipped `{}`: {error}",
            crate::input::display_path(path)
        );
    }
}

pub fn render_models(format: Format) -> Result<()> {
    let models = model_table();

    match format {
        Format::Json => {
            #[derive(Serialize)]
            struct Row<'a> {
                model: &'a str,
                encoding: &'a str,
                context_window: Option<usize>,
            }
            let rows: Vec<_> = models
                .iter()
                .map(|m| Row {
                    model: m.name,
                    encoding: m.encoding,
                    context_window: m.context_window,
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Format::Csv | Format::Tsv => {
            let sep = if format == Format::Csv { ',' } else { '\t' };
            println!("model{sep}encoding{sep}context_window");
            for m in &models {
                let window = m.context_window.map(|w| w.to_string()).unwrap_or_default();
                println!("{}{sep}{}{sep}{}", m.name, m.encoding, window);
            }
        }
        Format::Table => {
            let name_width = models.iter().map(|m| m.name.len()).max().unwrap_or(5);
            let enc_width = models.iter().map(|m| m.encoding.len()).max().unwrap_or(8);
            println!(
                "{DIM}{:<name_width$}  {:<enc_width$}  {:>9}{DIM:#}",
                "MODEL", "ENCODING", "CONTEXT"
            );
            for m in &models {
                let window = m
                    .context_window
                    .map(|w| thousands(w as u64))
                    .unwrap_or_else(|| "—".to_string());
                println!(
                    "{:<name_width$}  {DIM}{:<enc_width$}{DIM:#}  {:>9}",
                    m.name, m.encoding, window
                );
            }
            println!();
            println!(
                "{DIM}Any model name tiktoken knows works, including `ft:` fine-tunes.{DIM:#}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(12_345), "12,345");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn human_bytes_scales_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(10 * 1024 * 1024), "10.0 MiB");
    }

    #[test]
    fn bars_are_full_width_and_never_vanish() {
        assert_eq!(bar(10, 10).chars().count(), BAR_WIDTH);
        assert_eq!(bar(0, 10), "░".repeat(BAR_WIDTH));
        assert_eq!(bar(10, 10), "█".repeat(BAR_WIDTH));
        // A row that rounds to zero still shows one block.
        assert!(bar(1, 100_000).starts_with('█'));
    }

    #[test]
    fn context_line_switches_to_a_multiplier_past_the_window() {
        // Pinned to a model rather than the default, so changing the default
        // does not churn this test.
        let enc = Encoder::resolve(Some("gpt-4o"), None, false).unwrap(); // 128k window
        assert!(context_line(12_800, &enc).contains("10.0% of 128,000 context"));
        assert!(context_line(1_280_000, &enc).contains("10.0× the 128,000 token context"));
    }

    #[test]
    fn context_line_drops_a_fraction_that_rounds_to_zero() {
        let enc = Encoder::resolve(Some("gpt-4o"), None, false).unwrap(); // 128k window
        assert_eq!(context_line(2, &enc), "gpt-4o (o200k_base)");
        assert!(context_line(64, &enc).contains("0.1% of"));
    }

    #[test]
    fn context_line_is_omitted_for_a_bare_encoding() {
        let enc = Encoder::resolve(None, Some(crate::encoding::EncodingArg::Gpt2), false).unwrap();
        assert_eq!(context_line(999, &enc), "gpt2");
    }

    #[test]
    fn file_counts_are_grouped() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(3_349, "file"), "3,349 files");
    }

    #[test]
    fn csv_fields_are_quoted_only_when_needed() {
        assert_eq!(escape("src/main.rs", ','), "src/main.rs");
        assert_eq!(escape("a,b.rs", ','), "\"a,b.rs\"");
        assert_eq!(escape("a\"b.rs", ','), "\"a\"\"b.rs\"");
        assert_eq!(escape("a,b.rs", '\t'), "a,b.rs");
    }
}
