//! Resolving a model name or an explicit encoding into a usable BPE.

use anyhow::{Result, anyhow};
use clap::ValueEnum;
use tiktoken_rs::model::get_context_size;
use tiktoken_rs::tokenizer::{Tokenizer, get_tokenizer};
use tiktoken_rs::{CoreBPE, Rank, bpe_for_tokenizer};

/// Model assumed when the user names neither a model nor an encoding.
const DEFAULT_MODEL: &str = "gpt-5";

/// Encodings that can be selected directly with `--encoding`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum EncodingArg {
    #[value(name = "o200k_base", alias = "o200k-base")]
    O200kBase,
    #[value(name = "cl100k_base", alias = "cl100k-base")]
    Cl100kBase,
    #[value(name = "p50k_base", alias = "p50k-base")]
    P50kBase,
    #[value(name = "p50k_edit", alias = "p50k-edit")]
    P50kEdit,
    #[value(name = "r50k_base", alias = "r50k-base")]
    R50kBase,
    #[value(name = "o200k_harmony", alias = "o200k-harmony")]
    O200kHarmony,
    #[value(name = "gpt2")]
    Gpt2,
}

impl From<EncodingArg> for Tokenizer {
    fn from(arg: EncodingArg) -> Self {
        match arg {
            EncodingArg::O200kBase => Tokenizer::O200kBase,
            EncodingArg::Cl100kBase => Tokenizer::Cl100kBase,
            EncodingArg::P50kBase => Tokenizer::P50kBase,
            EncodingArg::P50kEdit => Tokenizer::P50kEdit,
            EncodingArg::R50kBase => Tokenizer::R50kBase,
            EncodingArg::O200kHarmony => Tokenizer::O200kHarmony,
            EncodingArg::Gpt2 => Tokenizer::Gpt2,
        }
    }
}

/// Canonical tiktoken name of an encoding, as used by OpenAI's Python library.
fn encoding_name(tokenizer: Tokenizer) -> &'static str {
    match tokenizer {
        Tokenizer::O200kHarmony => "o200k_harmony",
        Tokenizer::O200kBase => "o200k_base",
        Tokenizer::Cl100kBase => "cl100k_base",
        Tokenizer::P50kBase => "p50k_base",
        Tokenizer::R50kBase => "r50k_base",
        Tokenizer::P50kEdit => "p50k_edit",
        Tokenizer::Gpt2 => "gpt2",
    }
}

/// A resolved tokenizer plus the metadata we want to report alongside counts.
///
/// `CoreBPE` holds the whole vocabulary, so `Debug` prints the metadata only.
pub struct Encoder {
    /// `None` when the user selected an encoding directly.
    model: Option<String>,
    tokenizer: Tokenizer,
    bpe: &'static CoreBPE,
    context_window: Option<usize>,
    special: bool,
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encoder")
            .field("model", &self.model)
            .field("encoding", &self.encoding())
            .field("context_window", &self.context_window)
            .field("special", &self.special)
            .finish()
    }
}

impl Encoder {
    /// Resolve from the mutually exclusive `--model` / `--encoding` pair.
    pub fn resolve(
        model: Option<&str>,
        encoding: Option<EncodingArg>,
        special: bool,
    ) -> Result<Self> {
        if let Some(encoding) = encoding {
            let tokenizer = Tokenizer::from(encoding);
            return Ok(Self {
                model: None,
                tokenizer,
                bpe: bpe_for_tokenizer(tokenizer)?,
                context_window: None,
                special,
            });
        }

        let model = model.unwrap_or(DEFAULT_MODEL);
        let tokenizer = get_tokenizer(model).ok_or_else(|| unknown_model(model))?;
        Ok(Self {
            model: Some(model.to_string()),
            tokenizer,
            bpe: bpe_for_tokenizer(tokenizer)?,
            context_window: get_context_size(model),
            special,
        })
    }

    pub fn encode(&self, text: &str) -> Vec<Rank> {
        if self.special {
            self.bpe.encode_with_special_tokens(text)
        } else {
            self.bpe.encode_ordinary(text)
        }
    }

    pub fn count(&self, text: &str) -> usize {
        self.encode(text).len()
    }

    pub fn decode(&self, ids: &[Rank]) -> Result<Vec<u8>> {
        Ok(self.bpe.decode_bytes(ids)?)
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn encoding(&self) -> &'static str {
        encoding_name(self.tokenizer)
    }

    pub fn context_window(&self) -> Option<usize> {
        self.context_window
    }

    /// `gpt-4o (o200k_base)`, or just `o200k_base` for a direct encoding.
    pub fn label(&self) -> String {
        match &self.model {
            Some(model) => format!("{model} ({})", self.encoding()),
            None => self.encoding().to_string(),
        }
    }
}

fn unknown_model(model: &str) -> anyhow::Error {
    let hint = KNOWN_MODELS
        .iter()
        .find(|known| known.starts_with(model) || model.starts_with(*known))
        .map(|known| format!("\n  did you mean `{known}`?"))
        .unwrap_or_default();
    anyhow!(
        "unknown model `{model}`{hint}\n  \
         run `tok models` for the built-in list, or pass --encoding to pick an encoding directly"
    )
}

/// Models shown by `tok models`. Any name tiktoken understands is accepted,
/// this is just the curated list worth advertising.
pub const KNOWN_MODELS: &[&str] = &[
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2",
    "gpt-5.1-codex",
    "gpt-5",
    "gpt-5-mini",
    "gpt-5-nano",
    "codex-mini",
    "gpt-4.1",
    "gpt-4.1-mini",
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "gpt-4",
    "gpt-4-32k",
    "gpt-3.5-turbo",
    "o1",
    "o3",
    "o4-mini",
    "gpt-oss-120b",
    "gpt-oss-20b",
    "text-embedding-3-large",
    "text-embedding-3-small",
    "text-embedding-ada-002",
    "gpt2",
];

/// One row of `tok models`.
pub struct ModelInfo {
    pub name: &'static str,
    pub encoding: &'static str,
    pub context_window: Option<usize>,
}

pub fn model_table() -> Vec<ModelInfo> {
    KNOWN_MODELS
        .iter()
        .filter_map(|name| {
            let tokenizer = get_tokenizer(name)?;
            Some(ModelInfo {
                name,
                encoding: encoding_name(tokenizer),
                context_window: get_context_size(name),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_model_resolves() {
        assert_eq!(model_table().len(), KNOWN_MODELS.len());
    }

    #[test]
    fn default_model_uses_o200k() {
        let enc = Encoder::resolve(None, None, false).unwrap();
        assert_eq!(enc.encoding(), "o200k_base");
        assert_eq!(enc.model(), Some(DEFAULT_MODEL));
        assert_eq!(enc.context_window(), Some(400_000));
    }

    #[test]
    fn explicit_encoding_has_no_model_or_context() {
        let enc = Encoder::resolve(None, Some(EncodingArg::Cl100kBase), false).unwrap();
        assert_eq!(enc.encoding(), "cl100k_base");
        assert_eq!(enc.model(), None);
        assert_eq!(enc.context_window(), None);
        assert_eq!(enc.label(), "cl100k_base");
    }

    #[test]
    fn special_tokens_are_literal_by_default() {
        let plain = Encoder::resolve(None, None, false).unwrap();
        let special = Encoder::resolve(None, None, true).unwrap();
        assert!(plain.count("<|endoftext|>") > special.count("<|endoftext|>"));
        assert_eq!(special.count("<|endoftext|>"), 1);
    }

    #[test]
    fn unknown_model_is_an_error() {
        let msg = Encoder::resolve(Some("llama-3"), None, false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("unknown model `llama-3`"), "{msg}");
        assert!(msg.contains("tok models"), "{msg}");
    }

    #[test]
    fn a_partial_model_name_suggests_a_real_one() {
        let msg = Encoder::resolve(Some("gpt-"), None, false)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("did you mean"), "{msg}");
    }

    #[test]
    fn dated_and_fine_tuned_names_resolve_by_prefix() {
        for model in ["gpt-4o-2024-05-13", "ft:gpt-4o:acme:tuned:1", "gpt-4-0314"] {
            assert!(
                Encoder::resolve(Some(model), None, false).is_ok(),
                "{model} should resolve"
            );
        }
    }

    #[test]
    fn round_trips_through_decode() {
        let enc = Encoder::resolve(None, None, false).unwrap();
        let text = "hello world";
        let ids = enc.encode(text);
        assert_eq!(enc.decode(&ids).unwrap(), text.as_bytes());
    }
}
