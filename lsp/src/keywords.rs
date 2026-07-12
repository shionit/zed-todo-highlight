use lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct Keyword {
    pub pattern: Regex,
    pub token_type_index: u32,
    pub token_modifiers_bitset: u32,
}

pub fn build_keywords() -> (Vec<Keyword>, SemanticTokensLegend) {
    // Every keyword is emitted as the standard `keyword` token type so that
    // Zed's built-in default rule (`keyword` → theme keyword style) highlights
    // it without any user-defined semantic_token_rules. Each keyword also
    // carries its own modifier (e.g. `todo`, `fixme`) so users can override
    // colors per keyword by matching on `token_modifiers`.
    let definitions: &[(&str, &str)] = &[
        ("TODO", "todo"),
        ("FIXME", "fixme"),
        ("HACK", "hack"),
        ("NOTE", "note"),
        ("INFO", "info"),
        ("WARN", "warn"),
        ("WARNING", "warning"),
        ("BUG", "bug"),
        ("XXX", "xxx"),
        ("DEPRECATED", "deprecated"),
    ];

    let mut token_modifiers: Vec<SemanticTokenModifier> = Vec::new();
    let mut keywords: Vec<Keyword> = Vec::new();

    for (i, (kw, modifier)) in definitions.iter().enumerate() {
        token_modifiers.push(SemanticTokenModifier::new(modifier));
        let pat = Regex::new(&format!(r"\b{}\b", regex::escape(kw))).unwrap();
        keywords.push(Keyword {
            pattern: pat,
            token_type_index: 0,
            token_modifiers_bitset: 1 << i,
        });
    }

    let legend = SemanticTokensLegend {
        token_types: vec![SemanticTokenType::KEYWORD],
        token_modifiers,
    };

    (keywords, legend)
}
