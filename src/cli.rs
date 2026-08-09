//! Command line surface for `tok`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::encoding::EncodingArg;

const AFTER_HELP: &str = "\x1b[1mEXAMPLES:\x1b[0m
  tok README.md                 Count a single file
  tok src/ docs/                Count directories (respects .gitignore)
  cat prompt.txt | tok          Count from stdin
  tok -t 'hello world' --show   Visualise how text is split into tokens
  tok . --format json           Machine readable output for scripts
  tok . --limit 200000          Exit 1 when the total blows the budget
  tok decode 3575 1495          Turn token IDs back into text

Run `tok models` to see the built-in models and their encodings.";

#[derive(Parser, Debug)]
#[command(
    name = "tok",
    version,
    about = "Fast OpenAI token counter for files, directories and stdin",
    long_about = "Count OpenAI (tiktoken) tokens in files, directories and stdin.\n\n\
                  With no PATH, tok reads stdin. Directories are walked recursively, \
                  honouring .gitignore and skipping binary files.",
    after_long_help = AFTER_HELP
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub count: CountArgs,

    /// When to use colour. Global, so it works before or after a subcommand.
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto, value_name = "WHEN", global = true)]
    pub color: ColorChoice,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Decode token IDs back into text
    Decode(DecodeArgs),

    /// List well-known models with their encoding and context window
    Models(ModelsArgs),

    /// Generate a shell completion script
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Args, Debug)]
pub struct CountArgs {
    /// Files or directories to count; `-` means stdin
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Count this literal string instead of reading files
    #[arg(short = 't', long, value_name = "TEXT", conflicts_with = "paths")]
    pub text: Option<String>,

    #[command(flatten)]
    pub encoder: EncoderArgs,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Table)]
    pub format: Format,

    /// Print only the total token count
    #[arg(short, long, conflicts_with_all = ["format", "show", "ids"])]
    pub quiet: bool,

    /// Print token IDs instead of counting
    #[arg(long, conflicts_with = "show")]
    pub ids: bool,

    /// Visualise token boundaries with colour
    #[arg(long)]
    pub show: bool,

    /// Exit with status 1 when the total exceeds this many tokens
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,

    /// Group the totals by file extension instead of by file
    #[arg(long)]
    pub by_ext: bool,

    /// Show only the N largest entries
    #[arg(short = 'n', long, value_name = "N")]
    pub top: Option<usize>,

    /// Sort order for the per-file table
    #[arg(long, value_enum, default_value_t = Sort::Tokens)]
    pub sort: Sort,

    /// Reverse the sort order
    #[arg(short = 'r', long)]
    pub reverse: bool,

    #[command(flatten)]
    pub walk: WalkArgs,

    /// Number of threads to use (0 = one per core)
    #[arg(short = 'j', long, default_value_t = 0, value_name = "N")]
    pub threads: usize,
}

#[derive(Args, Debug)]
pub struct EncoderArgs {
    /// Model whose encoding should be used [default: gpt-5]
    #[arg(short, long, env = "TOK_MODEL", value_name = "MODEL")]
    pub model: Option<String>,

    /// Use an encoding directly instead of deriving one from --model
    #[arg(
        short,
        long,
        value_enum,
        conflicts_with = "model",
        value_name = "ENCODING"
    )]
    pub encoding: Option<EncodingArg>,

    /// Parse special tokens such as <|endoftext|> instead of treating them as plain text
    #[arg(long)]
    pub special: bool,
}

#[derive(Args, Debug, Default)]
pub struct WalkArgs {
    /// Include hidden files and directories
    #[arg(long)]
    pub hidden: bool,

    /// Do not respect .gitignore and friends
    #[arg(long)]
    pub no_ignore: bool,

    /// Only count files with this extension (repeatable)
    #[arg(long, value_name = "EXT")]
    pub ext: Vec<String>,

    /// Only count paths matching this glob (repeatable)
    #[arg(long, value_name = "GLOB")]
    pub glob: Vec<String>,

    /// Skip paths matching this glob (repeatable)
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Follow symbolic links while walking directories
    #[arg(long)]
    pub follow: bool,
}

#[derive(Args, Debug)]
pub struct DecodeArgs {
    /// Token IDs to decode; omit to read them from stdin
    #[arg(value_name = "ID")]
    pub ids: Vec<String>,

    #[command(flatten)]
    pub encoder: EncoderArgs,
}

#[derive(Args, Debug)]
pub struct ModelsArgs {
    /// Output format
    #[arg(short, long, value_enum, default_value_t = Format::Table)]
    pub format: Format,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Aligned, human readable table
    Table,
    /// JSON object
    Json,
    /// Comma separated values with a header row
    Csv,
    /// Tab separated values with a header row
    Tsv,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Sort {
    /// Largest token count first
    Tokens,
    /// Alphabetical by path
    Path,
    /// Largest file size first
    Size,
    /// Keep discovery order
    None,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    pub fn apply(self) {
        let choice = match self {
            ColorChoice::Auto => anstream::ColorChoice::Auto,
            ColorChoice::Always => anstream::ColorChoice::Always,
            ColorChoice::Never => anstream::ColorChoice::Never,
        };
        choice.write_global();
    }
}
