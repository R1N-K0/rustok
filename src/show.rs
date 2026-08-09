//! Token-level views: `--show`, `--ids` and the `decode` subcommand.

use anstream::{print, println};
use anstyle::{Color, Style};
use anyhow::{Context, Result, bail};

use crate::cli::Format;
use crate::encoding::Encoder;
use crate::input::Item;
use crate::output::thousands;

const DIM: Style = Style::new().dimmed();
const BOLD: Style = Style::new().bold();

/// Background colours cycled across tokens, chosen to stay legible on both
/// light and dark terminals. The foreground is pinned to black so a terminal
/// with its own bright/dark scheme cannot invert the text into the background.
const PALETTE: [u8; 5] = [153, 150, 223, 218, 159];
const PALETTE_FG: u8 = 0;

/// Split a token stream into printable groups, each valid UTF-8 on its own.
///
/// A single token can hold a partial UTF-8 sequence — very common for emoji,
/// CJK and other multi-byte text — so tokens are merged until the accumulated
/// bytes decode cleanly. Without this, every such token renders as `�`.
fn group_tokens(enc: &Encoder, ids: &[u32]) -> Result<Vec<String>> {
    let mut groups = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();

    for &id in ids {
        bytes.extend_from_slice(&enc.decode(&[id])?);

        if let Ok(text) = std::str::from_utf8(&bytes) {
            groups.push(text.to_string());
            bytes.clear();
        }
    }

    // Trailing bytes that never completed a character: show them lossily rather
    // than dropping tokens from the display.
    if !bytes.is_empty() {
        groups.push(String::from_utf8_lossy(&bytes).into_owned());
    }

    Ok(groups)
}

/// Print text with each token on its own background colour.
pub fn show(enc: &Encoder, label: &str, text: &str, with_header: bool) -> Result<()> {
    let ids = enc.encode(text);
    let groups = group_tokens(enc, &ids)?;

    if with_header {
        println!("{DIM}── {label}{DIM:#}");
    }

    for (i, group) in groups.iter().enumerate() {
        let style = Style::new()
            .bg_color(Some(Color::Ansi256(PALETTE[i % PALETTE.len()].into())))
            .fg_color(Some(Color::Ansi256(PALETTE_FG.into())));

        // Reset before every newline, otherwise the background bleeds to the
        // end of the terminal line.
        let mut lines = group.split('\n');
        if let Some(first) = lines.next() {
            print!("{style}{first}{style:#}");
        }
        for line in lines {
            println!();
            print!("{style}{line}{style:#}");
        }
    }
    println!();

    println!(
        "{DIM}──{DIM:#} {BOLD}{}{BOLD:#} tokens {DIM}·{DIM:#} {} chars",
        thousands(ids.len() as u64),
        thousands(text.chars().count() as u64),
    );
    Ok(())
}

/// Print raw token IDs, one input at a time.
pub fn ids(
    enc: &Encoder,
    label: &str,
    text: &str,
    format: Format,
    with_header: bool,
) -> Result<()> {
    let ids = enc.encode(text);

    if format == Format::Json {
        println!("{}", serde_json::to_string(&ids)?);
        return Ok(());
    }

    if format == Format::Table && with_header {
        println!("{DIM}── {label}{DIM:#}");
    }

    let sep = match format {
        Format::Csv => ",",
        Format::Tsv => "\t",
        _ => " ",
    };
    let joined: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    println!("{}", joined.join(sep));
    Ok(())
}

/// Run `--show` or `--ids` over every collected input.
pub fn render_items(enc: &Encoder, items: &[Item], mode_show: bool, format: Format) -> Result<()> {
    let multiple = items.len() > 1;

    for item in items {
        let (label, text) = match item {
            Item::Inline { label, text } => (label.clone(), text.clone()),
            Item::File(path) => {
                let bytes = std::fs::read(path).with_context(|| {
                    format!("cannot read `{}`", crate::input::display_path(path))
                })?;
                (
                    crate::input::display_path(path),
                    String::from_utf8_lossy(&bytes).into_owned(),
                )
            }
        };

        if mode_show {
            show(enc, &label, &text, multiple)?;
        } else {
            ids(enc, &label, &text, format, multiple)?;
        }
    }
    Ok(())
}

/// `tok decode 3575 1495` — token IDs back into text.
pub fn decode(enc: &Encoder, raw: &[String]) -> Result<String> {
    let mut ids = Vec::new();
    for chunk in raw {
        // Accept `1 2 3`, `1,2,3` and `[1, 2, 3]` so pasting from a log works.
        for token in chunk
            .split([',', ' ', '\t', '\n', '[', ']'])
            .filter(|t| !t.is_empty())
        {
            ids.push(
                token
                    .parse::<u32>()
                    .with_context(|| format!("`{token}` is not a token ID"))?,
            );
        }
    }

    if ids.is_empty() {
        bail!("no token IDs given");
    }

    let bytes = enc
        .decode(&ids)
        .context("token IDs are not valid for this encoding")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{Encoder, EncodingArg};

    fn enc() -> Encoder {
        Encoder::resolve(None, Some(EncodingArg::O200kBase), false).unwrap()
    }

    #[test]
    fn ascii_groups_are_one_token_each() {
        let enc = enc();
        let ids = enc.encode("hello world");
        let groups = group_tokens(&enc, &ids).unwrap();
        // Every ASCII token decodes on its own, so nothing is merged.
        assert_eq!(groups.len(), ids.len());
        assert_eq!(groups.concat(), "hello world");
    }

    #[test]
    fn multibyte_text_never_renders_replacement_characters() {
        let enc = enc();
        let text = "こんにちは世界🎉";
        let ids = enc.encode(text);
        let groups = group_tokens(&enc, &ids).unwrap();

        let joined = groups.concat();
        assert_eq!(joined, text);
        assert!(!joined.contains('\u{fffd}'));
        // Merging happened: fewer groups than tokens, but no text was lost.
        assert!(groups.len() < ids.len());
    }

    #[test]
    fn decode_accepts_separators_from_logs() {
        let enc = enc();
        let ids = enc.encode("hello world");
        let joined = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        assert_eq!(decode(&enc, &[joined]).unwrap(), "hello world");
        assert_eq!(
            decode(&enc, &[format!("[{}]", ids[0])]).unwrap(),
            enc.decode(&ids[..1])
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap()
        );
    }

    #[test]
    fn decode_rejects_junk() {
        let enc = enc();
        assert!(decode(&enc, &["abc".to_string()]).is_err());
        assert!(decode(&enc, &[]).is_err());
    }

    #[test]
    fn decode_round_trips_japanese() {
        let enc = enc();
        let text = "日本語のトークン";
        let ids: Vec<String> = enc.encode(text).iter().map(|i| i.to_string()).collect();
        assert_eq!(decode(&enc, &ids).unwrap(), text);
    }
}
