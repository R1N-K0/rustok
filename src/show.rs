//! Token-level views: `--show`, `--ids` and the `decode` subcommand.

use anstream::{print, println};
use anstyle::{Color, Style};
use anyhow::{Context, Result, bail};

use crate::cli::Format;
use crate::encoding::Encoder;
use crate::input::Item;
use crate::output::thousands;

/// Background colours cycled across tokens. Chosen to stay legible on both
/// light and dark terminals, with an explicit foreground so neither inverts.
const PALETTE: [(u8, u8); 5] = [(153, 0), (150, 0), (223, 0), (218, 0), (159, 0)];

/// One contiguous run of tokens that forms valid UTF-8 on its own.
struct Group {
    text: String,
    tokens: usize,
}

/// Split a token stream into printable groups.
///
/// A single token can hold a partial UTF-8 sequence — very common for Japanese,
/// emoji and other multi-byte text — so tokens are merged until the accumulated
/// bytes decode cleanly. Without this, every such token renders as `�`.
fn group_tokens(enc: &Encoder, ids: &[u32]) -> Result<Vec<Group>> {
    let mut groups = Vec::new();
    let mut pending: Vec<u32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();

    for &id in ids {
        pending.push(id);
        bytes.extend_from_slice(&enc.decode(&[id])?);

        if let Ok(text) = std::str::from_utf8(&bytes) {
            groups.push(Group {
                text: text.to_string(),
                tokens: pending.len(),
            });
            pending.clear();
            bytes.clear();
        }
    }

    // Trailing bytes that never completed a character: show them lossily rather
    // than dropping tokens from the display.
    if !pending.is_empty() {
        groups.push(Group {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            tokens: pending.len(),
        });
    }

    Ok(groups)
}

/// Print text with each token on its own background colour.
pub fn show(enc: &Encoder, label: &str, text: &str, with_header: bool) -> Result<()> {
    let ids = enc.encode(text);
    let groups = group_tokens(enc, &ids)?;

    if with_header {
        let dim = Style::new().dimmed();
        println!("{dim}── {label}{dim:#}");
    }

    for (i, group) in groups.iter().enumerate() {
        let (bg, fg) = PALETTE[i % PALETTE.len()];
        let style = Style::new()
            .bg_color(Some(Color::Ansi256(bg.into())))
            .fg_color(Some(Color::Ansi256(fg.into())));

        // Reset before every newline, otherwise the background bleeds to the
        // end of the terminal line.
        let mut lines = group.text.split('\n');
        if let Some(first) = lines.next() {
            print!("{style}{first}{style:#}");
        }
        for line in lines {
            println!();
            print!("{style}{line}{style:#}");
        }
    }
    println!();

    let dim = Style::new().dimmed();
    let bold = Style::new().bold();
    let merged = groups.iter().filter(|g| g.tokens > 1).count();
    let note = if merged > 0 {
        format!(
            " {dim}({merged} multi-byte {} span more than one token){dim:#}",
            if merged == 1 {
                "character"
            } else {
                "characters"
            }
        )
    } else {
        String::new()
    };
    println!(
        "{dim}──{dim:#} {bold}{}{bold:#} tokens {dim}·{dim:#} {} chars{note}",
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

    match format {
        Format::Json => {
            println!("{}", serde_json::to_string(&ids)?);
        }
        Format::Csv | Format::Tsv => {
            let sep = if format == Format::Csv { "," } else { "\t" };
            let joined: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
            println!("{}", joined.join(sep));
        }
        Format::Table => {
            if with_header {
                let dim = Style::new().dimmed();
                println!("{dim}── {label}{dim:#}");
            }
            let joined: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
            println!("{}", joined.join(" "));
        }
    }
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
        assert_eq!(groups.len(), ids.len());
        assert!(groups.iter().all(|g| g.tokens == 1));
        let joined: String = groups.iter().map(|g| g.text.as_str()).collect();
        assert_eq!(joined, "hello world");
    }

    #[test]
    fn multibyte_text_never_renders_replacement_characters() {
        let enc = enc();
        let text = "こんにちは世界🎉";
        let ids = enc.encode(text);
        let groups = group_tokens(&enc, &ids).unwrap();

        let joined: String = groups.iter().map(|g| g.text.as_str()).collect();
        assert_eq!(joined, text);
        assert!(!joined.contains('\u{fffd}'));
        // Grouping must preserve the true token count.
        assert_eq!(groups.iter().map(|g| g.tokens).sum::<usize>(), ids.len());
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
