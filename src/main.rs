//! `tok` — count OpenAI (tiktoken) tokens in files, directories and stdin.

mod cli;
mod count;
mod encoding;
mod input;
mod output;
mod show;

use std::io::Write;
use std::process::ExitCode;

use anstream::eprintln;
use anstyle::{AnsiColor, Color, Style};
use anyhow::Result;
use clap::{CommandFactory, Parser};

use cli::{Cli, Command, CountArgs};
use encoding::Encoder;

/// Returned when `--limit` is exceeded, so scripts can tell a budget failure
/// apart from a real error.
const EXIT_OVER_LIMIT: u8 = 1;
const EXIT_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            let style = Style::new()
                .bold()
                .fg_color(Some(Color::Ansi(AnsiColor::Red)));
            eprintln!("{style}error{style:#}: {err}");
            for cause in err.chain().skip(1) {
                eprintln!("  caused by: {cause}");
            }
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    cli.color.apply();

    match cli.command {
        Some(Command::Completions { shell }) => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Models(args)) => {
            output::render_models(args.format)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Decode(args)) => {
            let enc = Encoder::resolve(
                args.encoder.model.as_deref(),
                args.encoder.encoding,
                args.encoder.special,
            )?;
            let raw = if args.ids.is_empty() {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                vec![buf]
            } else {
                args.ids
            };
            let text = show::decode(&enc, &raw)?;
            // Write raw bytes: decoded text is data, not a message.
            std::io::stdout().write_all(text.as_bytes())?;
            std::io::stdout().flush()?;
            Ok(ExitCode::SUCCESS)
        }
        None => run_count(cli.count),
    }
}

fn run_count(args: CountArgs) -> Result<ExitCode> {
    let enc = Encoder::resolve(
        args.encoder.model.as_deref(),
        args.encoder.encoding,
        args.encoder.special,
    )?;

    let items = input::collect(&args.paths, args.text.as_deref(), &args.walk)?;

    if args.show || args.ids {
        show::render_items(&enc, &items, args.show, args.format)?;
        return Ok(ExitCode::SUCCESS);
    }

    let report = count::count_all(items, &enc, args.threads)?;
    output::render(&report, &enc, &args)?;

    if let Some(limit) = args.limit {
        let total = report.total_tokens();
        if total > limit {
            let style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)));
            eprintln!(
                "{style}over limit{style:#}: {} tokens exceeds the limit of {} (+{})",
                output::thousands(total as u64),
                output::thousands(limit as u64),
                output::thousands((total - limit) as u64),
            );
            return Ok(ExitCode::from(EXIT_OVER_LIMIT));
        }
    }

    Ok(ExitCode::SUCCESS)
}
