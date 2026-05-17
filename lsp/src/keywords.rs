use lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct Keyword {
    pub pattern: Regex,
    pub token_type_index: u32,
}

pub fn build_keywords() -> (Vec<Keyword>, SemanticTokensLegend) {
    let definitions: &[(&str, &str)] = &[
        ("TODO", "todoKeyword"),
        ("FIXME", "fixmeKeyword"),
        ("HACK", "hackKeyword"),
        ("NOTE", "noteKeyword"),
        ("INFO", "infoKeyword"),
        ("WARN", "warnKeyword"),
        ("WARNING", "warningKeyword"),
        ("BUG", "bugKeyword"),
        ("XXX", "xxxKeyword"),
        ("DEPRECATED", "deprecatedKeyword"),
    ];

    let mut token_types: Vec<SemanticTokenType> = Vec::new();
    let mut keywords: Vec<Keyword> = Vec::new();

    for (i, (kw, type_name)) in definitions.iter().enumerate() {
        token_types.push(SemanticTokenType::new(type_name));
        let pat = Regex::new(&format!(r"\b{}\b", regex::escape(kw))).unwrap();
        keywords.push(Keyword {
            pattern: pat,
            token_type_index: i as u32,
        });
    }

    let legend = SemanticTokensLegend {
        token_types,
        token_modifiers: vec![SemanticTokenModifier::new("none")],
    };

    (keywords, legend)
}
