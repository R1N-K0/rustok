# rustok

**Count OpenAI tokens in files, directories and stdin — fast.**
Installs as the `tok` command.

[![CI](https://github.com/R1N-K0/rustok/actions/workflows/ci.yml/badge.svg)](https://github.com/R1N-K0/rustok/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rustok.svg)](https://crates.io/crates/rustok)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`wc` counts bytes, words and lines. Models charge by *token*, and refuse work past a
context window. `tok` is the missing `wc` for that unit — point it at a repository and
find out what actually fits.

![tok in action](https://raw.githubusercontent.com/R1N-K0/rustok/main/assets/demo.gif)

```console
$ tok src/
TOKENS                FILE
 3,564  ████████████  src/output.rs
 2,257  ████████░░░░  src/encoding.rs
 1,803  ██████░░░░░░  src/show.rs
 1,511  █████░░░░░░░  src/cli.rs
 1,380  █████░░░░░░░  src/count.rs
 1,307  ████░░░░░░░░  src/input.rs
   787  ███░░░░░░░░░  src/main.rs
──────────────────────────────────────────────
12,609  tokens in 7 files · 48.0 KiB
        gpt-5 (o200k_base) · 3.2% of 400,000 context
```

Directories are walked recursively, `.gitignore` is honoured, binary files are skipped,
and files are read in parallel.

---

## Install

```sh
cargo install rustok
```

The project is `rustok`; the command it installs is `tok`, which is what you type.
(Same split as `fd-find`/`fd` and `du-dust`/`dust` — the short name was already taken
on crates.io.)

Or grab a prebuilt binary for Linux, macOS or Windows from the
[releases page](https://github.com/R1N-K0/rustok/releases).

## Usage

```console
$ tok README.md                    # one file
$ tok src/ docs/                   # directories, recursively
$ cat prompt.txt | tok             # stdin
$ tok -t 'hello world'             # a literal string
$ tok                              # no args: reads stdin
```

Every count says which encoding produced it, and — once the total is big enough to
register — how much of the model's context window it would fill:

```console
$ tok -t 'hello world'
2 tokens · gpt-5 (o200k_base)

$ tok src/main.rs
787 tokens · gpt-5 (o200k_base) · 0.2% of 400,000 context
```

### Pick a model or an encoding

The default is `gpt-5`. Any model name tiktoken knows works:

```console
$ tok -m gpt-4o src/               # a different model
$ tok -e cl100k_base src/          # or an encoding directly
$ TOK_MODEL=gpt-4.1 tok src/       # or set it once
$ tok models                       # what's available
```

Dated and fine-tuned names work too — `gpt-4o-2024-05-13`, `ft:gpt-4o:acme:tuned:1`.

### See the tokens, not just the count

`--show` paints each token in its own colour. Multi-byte text is handled properly:
a token holding only part of an emoji or accented character is merged with its
neighbour instead of rendering as `�`.

```console
$ tok -t 'Ship it 🚀' --show
Ship it 🚀
── 4 tokens · 9 chars
```

```console
$ tok -t 'hello world' --ids
24912 2375

$ tok decode 24912 2375
hello world
```

### Machine readable output

```console
$ tok src/ --format json
{
  "model": "gpt-5",
  "encoding": "o200k_base",
  "context_window": 400000,
  "total_tokens": 12609,
  "total_bytes": 49107,
  "context_used": 0.032,
  "files": [
    { "path": "src/output.rs", "tokens": 3564, "bytes": 13383, "lines": 421 }
  ],
  "skipped_binary": [],
  "failed": []
}
```

`--format csv` and `--format tsv` emit a header row plus a `TOTAL` row. `--quiet`
prints the bare number and nothing else:

```console
$ tok src/ --quiet
12609
```

### Guard a budget in CI

`--limit` exits **1** when the total is over budget, so it drops straight into a
pre-commit hook or a CI job:

```console
$ tok prompts/ --limit 8000
...
over limit: 12,609 tokens (limit 8,000, +4,609)
$ echo $?
1
```

```yaml
# .github/workflows/prompt-budget.yml
- run: tok prompts/ --limit 8000
```

### Find what is eating the context

```console
$ tok . -n 10                      # ten biggest files
$ tok . --by-ext                   # group by extension
$ tok . --ext rs --ext toml        # only these extensions
$ tok . --exclude 'target/*'       # skip a glob
$ tok . --sort path                # or size, tokens, none
```

## Options

| Flag | Meaning |
| --- | --- |
| `-m, --model <MODEL>` | Model whose encoding to use (default `gpt-5`, env `TOK_MODEL`) |
| `-e, --encoding <ENC>` | Use an encoding directly: `o200k_base`, `cl100k_base`, `p50k_base`, `p50k_edit`, `r50k_base`, `o200k_harmony`, `gpt2` |
| `-t, --text <TEXT>` | Count a literal string instead of files |
| `-f, --format <FMT>` | `table` (default), `json`, `csv`, `tsv` |
| `-q, --quiet` | Print only the total |
| `--ids` | Print token IDs |
| `--show` | Colourise token boundaries |
| `--limit <N>` | Exit 1 when the total exceeds `N` |
| `--by-ext` | Group totals by file extension |
| `-n, --top <N>` | Show only the `N` largest rows |
| `--sort <KEY>` | `tokens` (default), `path`, `size`, `none` |
| `-r, --reverse` | Reverse the sort |
| `--ext <EXT>` | Only count this extension (repeatable) |
| `--glob <GLOB>` | Only count matching paths (repeatable) |
| `--exclude <GLOB>` | Skip matching paths (repeatable) |
| `--hidden` | Include hidden files |
| `--no-ignore` | Ignore `.gitignore` and friends |
| `--follow` | Follow symlinks |
| `--special` | Parse `<\|endoftext\|>` as one token instead of literal text |
| `-j, --threads <N>` | Thread count (`0` = one per core) |
| `--color <WHEN>` | `auto`, `always`, `never` |

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | `--limit` exceeded |
| `2` | Error (unknown model, unreadable path, bad arguments) |

### Shell completions

```sh
tok completions bash > /etc/bash_completion.d/tok
tok completions zsh  > ~/.zfunc/_tok
tok completions fish > ~/.config/fish/completions/tok.fish
tok completions powershell | Out-String | Invoke-Expression
```

## Notes on accuracy

- Counts come from [`tiktoken-rs`](https://github.com/zurawiki/tiktoken-rs), a port of
  OpenAI's own [`tiktoken`](https://github.com/openai/tiktoken). For the same text and
  encoding the numbers match.
- `tok` counts **text**. A chat request also spends a handful of tokens per message on
  role and formatting overhead, so a real API call costs slightly more than the number
  here.
- Special tokens are treated as literal text by default, which matches how the API
  handles user content. Pass `--special` to tokenise them as single tokens.
- Non-UTF-8 bytes are decoded lossily rather than failing the whole run.
- Only OpenAI encodings are supported. Counts for other vendors' models are not
  comparable, and `tok` deliberately does not pretend otherwise.

## License

Dual licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
