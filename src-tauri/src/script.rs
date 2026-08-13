use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub const SCRIPT_DOCUMENT_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDocumentV1 {
    pub version: u8,
    pub blocks: Vec<ScriptBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScriptBlock {
    Paragraph { children: Vec<InlineNode> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InlineNode {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emphasis: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery: Option<Delivery>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<Origin>,
    },
    Pause {
        duration_ms: u32,
        origin: Origin,
    },
    Protected {
        text: String,
        kind: ProtectedKind,
        canonical: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pronunciation: Option<String>,
        origin: Origin,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Translation,
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Natural,
    Professional,
    Storytelling,
    Casual,
    Focus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedKind {
    Term,
    Number,
    Url,
    Code,
    Product,
}

impl ScriptDocumentV1 {
    pub fn from_plain_text(text: impl Into<String>, origin: Origin) -> Self {
        Self {
            version: SCRIPT_DOCUMENT_VERSION,
            blocks: vec![ScriptBlock::Paragraph {
                children: vec![InlineNode::Text {
                    text: text.into(),
                    emphasis: None,
                    delivery: None,
                    origin: Some(origin),
                }],
            }],
        }
    }

    pub fn parse_or_fallback(raw: Option<&str>, spoken_zh: &str) -> Self {
        raw.filter(|value| !value.trim().is_empty())
            .and_then(|value| serde_json::from_str::<Self>(value).ok())
            .filter(|document| document.validate().is_ok())
            .unwrap_or_else(|| Self::from_plain_text(spoken_zh, Origin::Translation))
    }

    pub fn has_manual_nodes(&self) -> bool {
        self.blocks.iter().any(|block| {
            let ScriptBlock::Paragraph { children } = block;
            children.iter().any(|node| match node {
                InlineNode::Text { origin, .. } => origin == &Some(Origin::Manual),
                InlineNode::Pause { origin, .. } | InlineNode::Protected { origin, .. } => {
                    *origin == Origin::Manual
                }
            })
        })
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.version != SCRIPT_DOCUMENT_VERSION {
            return Err(AppError::Validation(format!(
                "不支持的口播稿版本：{}",
                self.version
            )));
        }
        if self.blocks.is_empty() {
            return Err(AppError::Validation("口播稿至少需要一个段落".into()));
        }

        let mut visible_chars = 0usize;
        for block in &self.blocks {
            let ScriptBlock::Paragraph { children } = block;
            if children.is_empty() {
                return Err(AppError::Validation("口播稿段落不能为空".into()));
            }
            for node in children {
                match node {
                    InlineNode::Text { text, emphasis, .. } => {
                        if emphasis.is_some_and(|value| value > 2) {
                            return Err(AppError::Validation("强调强度只能是 0–2".into()));
                        }
                        visible_chars += text.chars().count();
                    }
                    InlineNode::Pause { duration_ms, .. } => {
                        if !(80..=1_200).contains(duration_ms) {
                            return Err(AppError::Validation(
                                "停顿时长需要在 80–1200ms 之间".into(),
                            ));
                        }
                    }
                    InlineNode::Protected {
                        text,
                        canonical,
                        pronunciation,
                        ..
                    } => {
                        if text.trim().is_empty() || canonical.trim().is_empty() {
                            return Err(AppError::Validation("受保护内容不能为空".into()));
                        }
                        if pronunciation
                            .as_ref()
                            .is_some_and(|value| value.len() > 256)
                        {
                            return Err(AppError::Validation("术语读音设置过长".into()));
                        }
                        visible_chars += text.chars().count();
                    }
                }
            }
        }
        if visible_chars == 0 {
            return Err(AppError::Validation("口播稿不能为空".into()));
        }
        if visible_chars > 10_000 {
            return Err(AppError::Validation(
                "单个片段口播稿不能超过 10000 字".into(),
            ));
        }
        Ok(())
    }

    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| {
                let ScriptBlock::Paragraph { children } = block;
                children
                    .iter()
                    .filter_map(|node| match node {
                        InlineNode::Text { text, .. } | InlineNode::Protected { text, .. } => {
                            Some(text.as_str())
                        }
                        InlineNode::Pause { .. } => None,
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Renders only the subset officially supported by iFlytek's public API.
    /// Style and emphasis stay in the application-level director layer; pauses
    /// become iFlytek's `[pNNN]` markers and protected text remains verbatim.
    pub fn render_iflytek_text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| {
                let ScriptBlock::Paragraph { children } = block;
                children
                    .iter()
                    .map(|node| match node {
                        InlineNode::Text {
                            text,
                            emphasis,
                            delivery,
                            ..
                        } => render_directed_phrase(text, *emphasis, *delivery),
                        InlineNode::Protected { text, .. } => text.clone(),
                        InlineNode::Pause { duration_ms, .. } => format!("[p{duration_ms}]"),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Provider-neutral text renderer for engines that do not accept inline
    /// SSML. Pauses are preserved as punctuation and emphasized phrases gain a
    /// short lead-in break, so the visible scheme-three marks materially affect
    /// synthesis instead of becoming metadata-only.
    pub fn render_directed_text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| {
                let ScriptBlock::Paragraph { children } = block;
                children
                    .iter()
                    .map(|node| match node {
                        InlineNode::Text {
                            text,
                            emphasis,
                            delivery,
                            ..
                        } => render_directed_phrase(text, *emphasis, *delivery),
                        InlineNode::Pause { duration_ms, .. } => {
                            if *duration_ms >= 500 {
                                "。。".into()
                            } else if *duration_ms >= 260 {
                                "。".into()
                            } else {
                                "，".into()
                            }
                        }
                        InlineNode::Protected { text, .. } => text.clone(),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn director_instruction(&self, style: &str) -> String {
        let style_instruction = match style {
            "professional" => "采用专业、清晰、可信的技术讲解口吻，咬字准确，节奏稳定",
            "conversational" | "natural" => "采用自然口语表达，亲切但不过度活泼，避免播报腔",
            "documentary" | "storytelling" => "采用沉稳克制的纪录片旁白口吻，层次清楚",
            "upbeat" | "casual" => "采用轻快、有交流感的分享口吻，保持自然",
            "emphasis" | "focus" => "突出关键信息，重点词更有力度，但不要夸张",
            _ => "根据技术口播语义自动选择自然、专业且不疲劳的表达",
        };
        let emphasized = self
            .blocks
            .iter()
            .flat_map(|block| {
                let ScriptBlock::Paragraph { children } = block;
                children
            })
            .filter_map(|node| match node {
                InlineNode::Text {
                    text,
                    emphasis: Some(level),
                    ..
                } if *level > 0 => Some(text.trim()),
                _ => None,
            })
            .filter(|value| !value.is_empty())
            .take(6)
            .collect::<Vec<_>>();
        if emphasized.is_empty() {
            format!("{style_instruction}。严格照稿朗读，不新增或省略信息。")
        } else {
            format!(
                "{style_instruction}。自然强调“{}”。严格照稿朗读，不新增或省略信息。",
                emphasized.join("、")
            )
        }
    }
}

fn render_directed_phrase(text: &str, emphasis: Option<u8>, delivery: Option<Delivery>) -> String {
    let mut rendered = text.to_string();
    if emphasis.is_some_and(|level| level > 0) && !rendered.starts_with(['，', '、', '；']) {
        rendered.insert(0, '，');
    }
    if matches!(delivery, Some(Delivery::Natural | Delivery::Casual))
        && !rendered.ends_with(['，', '。', '！', '？'])
    {
        rendered.push('，');
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_round_trips_and_keeps_pause_out_of_plain_text() {
        let document = ScriptDocumentV1 {
            version: 1,
            blocks: vec![ScriptBlock::Paragraph {
                children: vec![
                    InlineNode::Text {
                        text: "先检索".into(),
                        emphasis: Some(1),
                        delivery: Some(Delivery::Professional),
                        origin: Some(Origin::Auto),
                    },
                    InlineNode::Pause {
                        duration_ms: 280,
                        origin: Origin::Manual,
                    },
                    InlineNode::Protected {
                        text: "RAG".into(),
                        kind: ProtectedKind::Term,
                        canonical: "RAG".into(),
                        pronunciation: None,
                        origin: Origin::Translation,
                    },
                ],
            }],
        };
        document.validate().unwrap();
        let encoded = serde_json::to_string(&document).unwrap();
        let decoded: ScriptDocumentV1 = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, document);
        assert_eq!(decoded.plain_text(), "先检索RAG");
        assert_eq!(decoded.render_iflytek_text(), "，先检索[p280]RAG");
        assert_eq!(decoded.render_directed_text(), "，先检索。RAG");
    }

    #[test]
    fn malformed_document_falls_back_to_compatible_spoken_text() {
        let document = ScriptDocumentV1::parse_or_fallback(Some("{bad"), "兼容口播稿");
        assert_eq!(document.plain_text(), "兼容口播稿");
        assert_eq!(document.version, SCRIPT_DOCUMENT_VERSION);
    }

    #[test]
    fn invalid_pause_is_rejected() {
        let document = ScriptDocumentV1 {
            version: 1,
            blocks: vec![ScriptBlock::Paragraph {
                children: vec![
                    InlineNode::Text {
                        text: "测试".into(),
                        emphasis: None,
                        delivery: None,
                        origin: None,
                    },
                    InlineNode::Pause {
                        duration_ms: 10,
                        origin: Origin::Manual,
                    },
                ],
            }],
        };
        assert!(document.validate().is_err());
    }
}
