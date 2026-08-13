use crate::script::{
    Delivery, InlineNode, Origin, ProtectedKind, ScriptBlock, ScriptDocumentV1,
    SCRIPT_DOCUMENT_VERSION,
};

#[derive(Debug, Clone)]
pub struct ProtectedTerm {
    pub surface: String,
    pub canonical: String,
    pub pronunciation: Option<String>,
    pub kind: ProtectedKind,
}

/// A deterministic first pass for the “automatic director”. It intentionally
/// does not rewrite meaning: it protects terms/numbers, marks key clauses and
/// inserts editable pauses. A cloud director may later propose a conversational
/// rewrite, but that proposal still has to pass the same canonical coverage
/// checks before it can replace this document.
pub fn direct_plain_text(text: &str, style: &str, terms: &[ProtectedTerm]) -> ScriptDocumentV1 {
    let mut nodes = tokenize_protected(text, terms);
    nodes = split_punctuation_pauses(nodes);
    apply_delivery(&mut nodes, style);
    ScriptDocumentV1 {
        version: SCRIPT_DOCUMENT_VERSION,
        blocks: vec![ScriptBlock::Paragraph { children: nodes }],
    }
}

pub fn canonical_coverage(
    before: &ScriptDocumentV1,
    after: &ScriptDocumentV1,
) -> Result<(), Vec<String>> {
    let expected = canonical_values(before);
    let actual = canonical_values(after);
    let missing = expected
        .into_iter()
        .filter(|value| !actual.iter().any(|candidate| candidate == value))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

fn canonical_values(document: &ScriptDocumentV1) -> Vec<String> {
    document
        .blocks
        .iter()
        .flat_map(|block| {
            let ScriptBlock::Paragraph { children } = block;
            children
        })
        .filter_map(|node| match node {
            InlineNode::Protected { canonical, .. } => Some(canonical.trim().to_lowercase()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn tokenize_protected(text: &str, terms: &[ProtectedTerm]) -> Vec<InlineNode> {
    let mut candidates = terms
        .iter()
        .filter(|term| !term.surface.trim().is_empty())
        .map(|term| {
            (
                term.surface.clone(),
                term.canonical.clone(),
                term.pronunciation.clone(),
                term.kind,
            )
        })
        .collect::<Vec<_>>();
    candidates.extend(detect_automatic_protected(text));
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0.len()));
    candidates.dedup_by(|left, right| left.0 == right.0);

    let mut matches: Vec<(usize, usize, String, String, Option<String>, ProtectedKind)> =
        Vec::new();
    for (surface, canonical, pronunciation, kind) in candidates {
        for (start, _) in text.match_indices(&surface) {
            let end = start + surface.len();
            if !matches.iter().any(|(existing_start, existing_end, ..)| {
                start < *existing_end && end > *existing_start
            }) {
                matches.push((
                    start,
                    end,
                    surface.clone(),
                    canonical.clone(),
                    pronunciation.clone(),
                    kind,
                ));
            }
        }
    }
    matches.sort_by_key(|item| item.0);

    let mut nodes = Vec::new();
    let mut cursor = 0;
    for (start, end, surface, canonical, pronunciation, kind) in matches {
        if start > cursor {
            push_text(&mut nodes, &text[cursor..start]);
        }
        nodes.push(InlineNode::Protected {
            text: surface,
            kind,
            canonical,
            pronunciation,
            origin: Origin::Auto,
        });
        cursor = end;
    }
    if cursor < text.len() {
        push_text(&mut nodes, &text[cursor..]);
    }
    if nodes.is_empty() {
        push_text(&mut nodes, text);
    }
    nodes
}

fn detect_automatic_protected(text: &str) -> Vec<(String, String, Option<String>, ProtectedKind)> {
    let mut values = Vec::new();
    let mut token_start = None;
    let chars = text.char_indices().collect::<Vec<_>>();
    for (index, (_, character)) in chars.iter().enumerate() {
        let protected_char = character.is_ascii_alphanumeric()
            || matches!(character, '.' | '%' | '-' | '_' | '/' | ':' | '+' | '#');
        if protected_char && token_start.is_none() {
            token_start = Some(index);
        }
        let at_end = index + 1 == chars.len();
        if (!protected_char || at_end) && token_start.is_some() {
            let start_index = token_start.take().unwrap();
            let end_index = if protected_char && at_end {
                index + 1
            } else {
                index
            };
            let start_byte = chars[start_index].0;
            let end_byte = chars.get(end_index).map_or(text.len(), |item| item.0);
            let token = text[start_byte..end_byte]
                .trim_matches(|character: char| matches!(character, '.' | '-' | '_' | '/' | ':'));
            if token.is_empty() {
                continue;
            }
            let contains_digit = token.chars().any(|character| character.is_ascii_digit());
            let ascii_letters = token
                .chars()
                .filter(|character| character.is_ascii_alphabetic())
                .count();
            let uppercase = token
                .chars()
                .filter(|character| character.is_ascii_uppercase())
                .count();
            let is_url =
                token.starts_with("http") || token.contains(".com") || token.contains(".cn");
            let should_protect = is_url
                || contains_digit
                || (ascii_letters >= 2 && uppercase == ascii_letters)
                || token.contains(['_', '#', '+']);
            if should_protect {
                values.push((
                    token.to_string(),
                    token.to_string(),
                    None,
                    if is_url {
                        ProtectedKind::Url
                    } else if contains_digit {
                        ProtectedKind::Number
                    } else if token.contains(['_', '#', '+']) {
                        ProtectedKind::Code
                    } else {
                        ProtectedKind::Term
                    },
                ));
            }
        }
    }
    values
}

fn split_punctuation_pauses(nodes: Vec<InlineNode>) -> Vec<InlineNode> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            InlineNode::Text {
                text,
                emphasis,
                delivery,
                origin,
            } => {
                let mut cursor = 0;
                for (index, character) in text.char_indices() {
                    let pause = match character {
                        '，' | '、' | ',' => Some(160),
                        '；' | ';' | '：' | ':' => Some(220),
                        '。' | '！' | '？' | '!' | '?' => Some(300),
                        _ => None,
                    };
                    let Some(duration_ms) = pause else { continue };
                    let end = index + character.len_utf8();
                    if end > cursor {
                        output.push(InlineNode::Text {
                            text: text[cursor..end].to_string(),
                            emphasis,
                            delivery,
                            origin,
                        });
                    }
                    if !matches!(output.last(), Some(InlineNode::Pause { .. })) {
                        output.push(InlineNode::Pause {
                            duration_ms,
                            origin: Origin::Auto,
                        });
                    }
                    cursor = end;
                }
                if cursor < text.len() {
                    output.push(InlineNode::Text {
                        text: text[cursor..].to_string(),
                        emphasis,
                        delivery,
                        origin,
                    });
                }
            }
            other => output.push(other),
        }
    }
    while matches!(output.last(), Some(InlineNode::Pause { .. })) {
        output.pop();
    }
    output
}

fn apply_delivery(nodes: &mut [InlineNode], style: &str) {
    let delivery = match style {
        "professional" => Delivery::Professional,
        "conversational" | "natural" => Delivery::Natural,
        "documentary" | "storytelling" => Delivery::Storytelling,
        "upbeat" | "casual" => Delivery::Casual,
        "emphasis" | "focus" => Delivery::Focus,
        _ => Delivery::Professional,
    };
    for node in nodes {
        if let InlineNode::Text {
            text,
            emphasis,
            delivery: current_delivery,
            ..
        } = node
        {
            *current_delivery = Some(delivery);
            if style == "emphasis"
                || ["重要", "关键", "核心", "必须", "注意"]
                    .iter()
                    .any(|marker| text.contains(marker))
            {
                *emphasis = Some(1);
            }
        }
    }
}

fn push_text(nodes: &mut Vec<InlineNode>, text: &str) {
    if !text.is_empty() {
        nodes.push(InlineNode::Text {
            text: text.to_string(),
            emphasis: None,
            delivery: None,
            origin: Some(Origin::Translation),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn director_protects_terms_numbers_and_adds_editable_pauses() {
        let document = direct_plain_text(
            "RAG 让准确率提升 12%，这是关键。",
            "professional",
            &[ProtectedTerm {
                surface: "RAG".into(),
                canonical: "retrieval augmented generation".into(),
                pronunciation: None,
                kind: ProtectedKind::Term,
            }],
        );
        document.validate().unwrap();
        assert_eq!(document.plain_text(), "RAG 让准确率提升 12%，这是关键。");
        let ScriptBlock::Paragraph { children } = &document.blocks[0];
        assert!(children.iter().any(|node| matches!(
            node,
            InlineNode::Protected { text, .. } if text == "RAG"
        )));
        assert!(children.iter().any(|node| matches!(
            node,
            InlineNode::Protected { text, .. } if text == "12%"
        )));
        assert!(children.iter().any(|node| matches!(
            node,
            InlineNode::Pause {
                duration_ms: 160,
                ..
            }
        )));
    }

    #[test]
    fn canonical_coverage_rejects_dropped_protected_values() {
        let before = direct_plain_text("版本 v3.5 已发布", "auto", &[]);
        let after = ScriptDocumentV1::from_plain_text("版本已经发布", Origin::Auto);
        assert!(canonical_coverage(&before, &after).is_err());
    }
}
