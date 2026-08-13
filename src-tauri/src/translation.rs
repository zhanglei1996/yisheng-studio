use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{domain::SegmentRecord, error::AppError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    content: String,
}

#[derive(Debug, Deserialize)]
struct TranslationPayload {
    segments: Vec<TranslatedSegment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranslatedSegment {
    id: String,
    subtitle_zh: String,
    spoken_zh: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticBeatInput<'a> {
    pub id: &'a str,
    pub start_ms: i64,
    pub end_ms: i64,
    pub segments: Vec<SemanticSourceSegment<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSourceSegment<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub subtitle_zh: &'a str,
    pub spoken_zh: &'a str,
}

#[derive(Debug, Deserialize)]
struct SemanticNarrationPayload {
    segments: Vec<SemanticNarrationSegment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SemanticNarrationSegment {
    id: String,
    spoken_zh: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticFitSegment<'a> {
    pub id: &'a str,
    pub spoken_zh: &'a str,
}

#[derive(Serialize)]
struct SourceSegment<'a> {
    id: &'a str,
    source: &'a str,
    duration_ms: i64,
}

pub async fn translate_batch(
    client: &reqwest::Client,
    config: &ProviderConfig,
    secret: &str,
    segments: &[SegmentRecord],
) -> Result<Vec<(String, String, String)>, AppError> {
    let source = segments
        .iter()
        .map(|segment| SourceSegment {
            id: &segment.id,
            source: &segment.source_text,
            duration_ms: segment.end_ms - segment.start_ms,
        })
        .collect::<Vec<_>>();
    let input = serde_json::to_string(&source)
        .map_err(|error| AppError::Provider(format!("无法组织翻译批次：{error}")))?;
    let request = serde_json::json!({
        "model": config.model,
        "temperature": 0.1,
        "max_tokens": 2048,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": format!("你是技术课程本地化编辑。逐条把每个 source 本身翻译为准确自然的简体中文，严禁把某条 source 的信息移动、延后或合并到另一条；即使 source 是不完整短语，也只翻译该短语。技术名词、产品名、代码、协议缩写按业界惯例保留。subtitleZh忠实完整；spokenZh适合口语配音，并尽量能在durationMs内读完。输入恰好有 {} 个片段，输出也必须恰好有 {} 个片段。只输出JSON对象：{{\"segments\":[{{\"id\":\"原id\",\"subtitleZh\":\"中文字幕\",\"spokenZh\":\"中文配音文案\"}}]}}。逐个原样复制所有id，不得漏掉、增加、改写、重复id或重新分段。", source.len(), source.len())},
            {"role": "user", "content": input}
        ]
    });
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let response = client
        .post(url)
        .bearer_auth(secret)
        .json(&request)
        .send()
        .await
        .map_err(|error| {
            AppError::Provider(if error.is_timeout() {
                "翻译请求超时".into()
            } else {
                "无法连接翻译服务".into()
            })
        })?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(AppError::Provider("API Key 无效或没有模型权限".into()));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(AppError::Provider("翻译服务正在限流，请稍后重试".into()));
    }
    if !status.is_success() {
        return Err(AppError::Provider(format!(
            "翻译服务返回 HTTP {}",
            status.as_u16()
        )));
    }
    let response: ChatResponse = response
        .json()
        .await
        .map_err(|_| AppError::Provider("翻译服务响应格式不正确".into()))?;
    let content = response
        .choices
        .first()
        .ok_or_else(|| AppError::Provider("翻译服务没有返回结果".into()))?
        .message
        .content
        .trim();
    parse_translation_content(content, segments)
}

/// Rewrites a 30–60 second scene as natural Chinese narration while keeping
/// the existing subtitle IDs as editable storage slots. The model may move or
/// condense information within a beat, but every original slot is returned so
/// the editor and undo/history model remain stable.
pub async fn rewrite_semantic_scene(
    client: &reqwest::Client,
    config: &ProviderConfig,
    secret: &str,
    beats: &[SemanticBeatInput<'_>],
) -> Result<Vec<(String, String)>, AppError> {
    let expected_ids = beats
        .iter()
        .flat_map(|beat| beat.segments.iter().map(|segment| segment.id))
        .collect::<Vec<_>>();
    if expected_ids.is_empty() {
        return Err(AppError::Validation("语义旁白场景没有可改写片段".into()));
    }
    let input = serde_json::to_string(beats)
        .map_err(|error| AppError::Provider(format!("无法组织语义旁白场景：{error}")))?;
    let request = serde_json::json!({
        "model": config.model,
        "temperature": 0.35,
        "max_tokens": 4096,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": format!("你是中文技术视频的旁白编导。先理解整个30到60秒场景，再用同一个讲述者自然地重新讲一遍，不要逐字逐句翻译。允许在同一个beat内合并、前移、后移或省略口头重复，只要事实和画面重点仍然对应；不得新增原文没有的事实。每个beat是必须重新对齐画面的语义锚点，口播必须能在它的startMs到endMs窗口内自然读完，中文正文建议每秒3到4个字。相邻beat要用自然承接语气，不要重复开场、报幕或总结。技术名词、协议名和关键数字必须准确。输入共有{}个原始segment，输出必须恰好{}个，逐个原样复制所有segment id；可跨segment重新分配措辞，但每个spokenZh都必须非空。字幕译文不在本步骤修改。只输出JSON对象：{{\"segments\":[{{\"id\":\"原id\",\"spokenZh\":\"自然中文旁白\"}}]}}。", expected_ids.len(), expected_ids.len())},
            {"role": "user", "content": input}
        ]
    });
    let response = client
        .post(format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        ))
        .bearer_auth(secret)
        .json(&request)
        .send()
        .await
        .map_err(|error| {
            AppError::Provider(if error.is_timeout() {
                "语义旁白改写超时".into()
            } else {
                "无法连接语义旁白改写服务".into()
            })
        })?;
    if !response.status().is_success() {
        return Err(AppError::Provider(format!(
            "语义旁白改写返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let response: ChatResponse = response
        .json()
        .await
        .map_err(|_| AppError::Provider("语义旁白改写响应格式不正确".into()))?;
    let raw = response
        .choices
        .first()
        .ok_or_else(|| AppError::Provider("语义旁白改写没有返回结果".into()))?
        .message
        .content
        .trim();
    let without_prefix = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```JSON"))
        .or_else(|| raw.strip_prefix("```"))
        .unwrap_or(raw);
    let clean = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let payload: SemanticNarrationPayload = serde_json::from_str(clean)
        .map_err(|_| AppError::Provider("模型没有返回约定的语义旁白结构".into()))?;
    let expected = expected_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let actual = payload
        .segments
        .iter()
        .map(|item| item.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if expected != actual || payload.segments.len() != expected.len() {
        return Err(AppError::Provider(
            "语义旁白返回的片段 ID 不完整，已停止写入".into(),
        ));
    }
    if payload
        .segments
        .iter()
        .any(|item| item.spoken_zh.trim().is_empty())
    {
        return Err(AppError::Provider("语义旁白返回了空白口播稿".into()));
    }
    Ok(payload
        .segments
        .into_iter()
        .map(|item| (item.id, item.spoken_zh.trim().to_string()))
        .collect())
}

/// Compresses one acoustic block as a whole. This lets the model remove
/// repetition across adjacent subtitle slots instead of shortening each row in
/// isolation, while the stable IDs keep local editing and subtitles intact.
pub async fn compress_semantic_block(
    client: &reqwest::Client,
    config: &ProviderConfig,
    secret: &str,
    segments: &[SemanticFitSegment<'_>],
    target_chars: usize,
) -> Result<Vec<(String, String)>, AppError> {
    if segments.is_empty() {
        return Err(AppError::Validation("语义配音块没有可压缩文案".into()));
    }
    let expected = segments
        .iter()
        .map(|segment| segment.id)
        .collect::<std::collections::HashSet<_>>();
    let input = serde_json::to_string(segments)
        .map_err(|error| AppError::Provider(format!("无法组织语义配音块：{error}")))?;
    let request = serde_json::json!({
        "model": config.model,
        "temperature": 0.15,
        "max_tokens": 768,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": format!("把这一整段连续中文旁白压缩到总计最多 {target_chars} 个字符，保证自然口语且保留关键事实、技术名词和数字。允许在相邻片段之间重新分配措辞并删除重复说明，但不能增加原文没有的信息。输出必须保留全部 {} 个 id、顺序不变，每个 spokenZh 非空；字幕不会被修改。只输出 JSON：{{\"segments\":[{{\"id\":\"原id\",\"spokenZh\":\"压缩后的自然口播\"}}]}}。", segments.len())},
            {"role": "user", "content": input}
        ]
    });
    let response = client
        .post(format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        ))
        .bearer_auth(secret)
        .json(&request)
        .send()
        .await
        .map_err(|_| AppError::Provider("无法连接语义配音块压缩服务".into()))?;
    if !response.status().is_success() {
        return Err(AppError::Provider(format!(
            "语义配音块压缩返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let response: ChatResponse = response
        .json()
        .await
        .map_err(|_| AppError::Provider("语义配音块压缩响应格式不正确".into()))?;
    let content = response
        .choices
        .first()
        .ok_or_else(|| AppError::Provider("语义配音块压缩没有返回结果".into()))?
        .message
        .content
        .trim();
    let without_prefix = content
        .strip_prefix("```json")
        .or_else(|| content.strip_prefix("```JSON"))
        .or_else(|| content.strip_prefix("```"))
        .unwrap_or(content);
    let clean = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let payload: SemanticNarrationPayload = serde_json::from_str(clean)
        .map_err(|_| AppError::Provider("模型没有返回约定的语义配音块结构".into()))?;
    let actual = payload
        .segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if actual != expected
        || payload.segments.len() != segments.len()
        || payload
            .segments
            .iter()
            .any(|segment| segment.spoken_zh.trim().is_empty())
    {
        return Err(AppError::Provider(
            "语义配音块压缩没有完整保留片段 ID".into(),
        ));
    }
    Ok(payload
        .segments
        .into_iter()
        .map(|segment| (segment.id, segment.spoken_zh.trim().to_string()))
        .collect())
}

fn parse_translation_content(
    content: &str,
    segments: &[SegmentRecord],
) -> Result<Vec<(String, String, String)>, AppError> {
    let trimmed = content.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```JSON"))
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let clean = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let payload: TranslationPayload = serde_json::from_str(clean)
        .map_err(|_| AppError::Provider("模型没有返回约定的结构化翻译".into()))?;
    let expected = segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut translated = payload
        .segments
        .into_iter()
        .map(|item| (item.id, item.subtitle_zh, item.spoken_zh))
        .collect::<Vec<_>>();
    if segments.len() == 1 && translated.len() == 1 {
        translated[0].0.clone_from(&segments[0].id);
    }
    let actual = translated
        .iter()
        .map(|item| item.0.as_str())
        .collect::<std::collections::HashSet<_>>();
    if expected != actual || translated.len() != segments.len() {
        return Err(AppError::Provider(
            "模型返回的片段 ID 不完整，已停止写入".into(),
        ));
    }
    if translated.iter().any(|(_, subtitle, spoken)| {
        subtitle
            .trim_matches(['。', '，', '！', '？', ' '])
            .is_empty()
            || spoken.trim().is_empty()
    }) {
        return Err(AppError::Provider("模型返回了空白片段翻译".into()));
    }
    Ok(translated)
}

pub fn is_retryable_format_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Provider(message)
            if message.contains("结构化翻译")
                || message.contains("片段 ID 不完整")
                || message.contains("没有返回结果")
                || message.contains("空白片段翻译")
    )
}

pub async fn compress_spoken(
    client: &reqwest::Client,
    config: &ProviderConfig,
    secret: &str,
    segment: &SegmentRecord,
    target_chars: usize,
) -> Result<String, AppError> {
    let request = serde_json::json!({
        "model": config.model,
        "temperature": 0.1,
        "max_tokens": 256,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": format!("把中文配音文案压缩到最多 {target_chars} 个汉字的口播时长。这个上限是硬约束：英文字母、数字和标点也计入长度，URL 或逐字母拼读会明显更慢。保留关键事实和技术含义，但字幕已经保留完整原文，因此极短片段允许省略寒暄、链接或把 HTTP/HTTPS、Chrome 等改成自然的中文语义（例如‘安全连接’、‘浏览器’）。优先使用短句，不能补充原文没有的信息。只输出 JSON：{{\"spokenZh\":\"压缩后的文案\"}}。")},
            {"role": "user", "content": serde_json::json!({"source": segment.source_text, "subtitleZh": segment.subtitle_zh, "spokenZh": segment.spoken_zh, "durationMs": segment.end_ms - segment.start_ms}).to_string()}
        ]
    });
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let response = client
        .post(url)
        .bearer_auth(secret)
        .json(&request)
        .send()
        .await
        .map_err(|_| AppError::Provider("无法连接配音文案压缩服务".into()))?;
    if !response.status().is_success() {
        return Err(AppError::Provider(format!(
            "配音文案压缩返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    let response: ChatResponse = response
        .json()
        .await
        .map_err(|_| AppError::Provider("配音文案压缩响应格式不正确".into()))?;
    let raw = response
        .choices
        .first()
        .ok_or_else(|| AppError::Provider("配音文案压缩没有返回结果".into()))?
        .message
        .content
        .trim();
    let without_prefix = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```"))
        .unwrap_or(raw);
    let clean = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let value: serde_json::Value = serde_json::from_str(clean)
        .map_err(|_| AppError::Provider("配音文案压缩没有返回 JSON".into()))?;
    let spoken = value
        .get("spokenZh")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Provider("配音文案压缩结果为空".into()))?;
    Ok(spoken.to_string())
}

pub async fn translate_one_fallback(
    client: &reqwest::Client,
    config: &ProviderConfig,
    secret: &str,
    segment: &SegmentRecord,
) -> Result<(String, String, String), AppError> {
    let request = serde_json::json!({
        "model": config.model,
        "temperature": 0.1,
        "max_tokens": 256,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": "把这一个英文短语或句子翻译成简体中文，只翻译输入本身。必须输出非空 JSON：{\"subtitleZh\":\"忠实翻译\",\"spokenZh\":\"自然配音文案\"}。"},
            {"role": "user", "content": segment.source_text}
        ]
    });
    let response = client
        .post(format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        ))
        .bearer_auth(secret)
        .json(&request)
        .send()
        .await
        .map_err(|_| AppError::Provider("无法连接单片段翻译服务".into()))?;
    let response: ChatResponse = response
        .json()
        .await
        .map_err(|_| AppError::Provider("单片段翻译响应格式不正确".into()))?;
    let raw = response
        .choices
        .first()
        .ok_or_else(|| AppError::Provider("单片段翻译没有返回结果".into()))?
        .message
        .content
        .trim();
    let without_prefix = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```"))
        .unwrap_or(raw);
    let clean = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    let value: serde_json::Value = serde_json::from_str(clean)
        .map_err(|_| AppError::Provider("单片段翻译没有返回 JSON".into()))?;
    let subtitle = value
        .get("subtitleZh")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Provider("单片段字幕为空".into()))?;
    let spoken = value
        .get("spokenZh")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Provider("单片段配音文案为空".into()))?;
    Ok((segment.id.clone(), subtitle.into(), spoken.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(id: &str) -> SegmentRecord {
        SegmentRecord {
            id: id.into(),
            project_id: "project".into(),
            ordinal: 0,
            start_ms: 0,
            end_ms: 1_000,
            source_text: "HTTP Strict Transport Security".into(),
            subtitle_zh: String::new(),
            spoken_zh: String::new(),
            linked: true,
            status: "asr_ready".into(),
            script_doc_json: String::new(),
            script_revision: 1,
            tts_overrides_json: "{}".into(),
            tts_state: "missing".into(),
            tts_error_message: None,
            tts_settings_hash: None,
            tts_duration_ms: None,
        }
    }

    #[test]
    fn parses_plain_and_fenced_structured_results() {
        let segments = vec![segment("seg-1")];
        for content in [
            r#"{"segments":[{"id":"seg-1","subtitleZh":"HTTP 严格传输安全","spokenZh":"HTTP 严格传输安全"}]}"#,
            "```json\n{\"segments\":[{\"id\":\"seg-1\",\"subtitleZh\":\"HTTP 严格传输安全\",\"spokenZh\":\"HTTP 严格传输安全\"}]}\n```",
        ] {
            let result = parse_translation_content(content, &segments).unwrap();
            assert_eq!(result[0].0, "seg-1");
            assert_eq!(result[0].1, "HTTP 严格传输安全");
        }
    }

    #[test]
    fn rejects_missing_or_unexpected_segment_ids() {
        let segments = vec![segment("seg-1"), segment("seg-2")];
        let error = parse_translation_content(
            r#"{"segments":[{"id":"seg-1","subtitleZh":"一","spokenZh":"一"},{"id":"other","subtitleZh":"二","spokenZh":"二"}]}"#,
            &segments,
        )
        .unwrap_err();
        assert!(error.to_string().contains("片段 ID 不完整"));
    }

    #[test]
    fn rejects_non_json_provider_output() {
        let error = parse_translation_content("这里是翻译结果", &[segment("seg-1")]).unwrap_err();
        assert!(is_retryable_format_error(&error));
        assert!(error.to_string().contains("结构化翻译"));
    }

    #[test]
    fn rejects_duplicate_ids_even_when_the_set_matches() {
        let segments = vec![segment("seg-1"), segment("seg-2")];
        let error = parse_translation_content(
            r#"{"segments":[{"id":"seg-1","subtitleZh":"一","spokenZh":"一"},{"id":"seg-1","subtitleZh":"重复","spokenZh":"重复"},{"id":"seg-2","subtitleZh":"二","spokenZh":"二"}]}"#,
            &segments,
        )
        .unwrap_err();
        assert!(error.to_string().contains("片段 ID 不完整"));
    }

    #[test]
    fn safely_remaps_a_single_item_fallback_to_the_requested_id() {
        let result = parse_translation_content(
            r#"{"segments":[{"id":"模型改写的-id","subtitleZh":"一条翻译","spokenZh":"一条配音"}]}"#,
            &[segment("stable-uuid")],
        )
        .unwrap();
        assert_eq!(result[0].0, "stable-uuid");
    }

    #[test]
    fn semantic_scene_input_keeps_visual_beat_boundaries() {
        let first = segment("seg-1");
        let second = segment("seg-2");
        let beat = SemanticBeatInput {
            id: "seg-1",
            start_ms: 1_000,
            end_ms: 8_000,
            segments: vec![
                SemanticSourceSegment {
                    id: &first.id,
                    source: &first.source_text,
                    subtitle_zh: "HTTP 严格传输安全",
                    spoken_zh: "先理解 HSTS",
                },
                SemanticSourceSegment {
                    id: &second.id,
                    source: &second.source_text,
                    subtitle_zh: "浏览器会强制安全连接",
                    spoken_zh: "浏览器强制安全连接",
                },
            ],
        };
        let value = serde_json::to_value(&beat).unwrap();
        assert_eq!(value["startMs"], 1_000);
        assert_eq!(value["endMs"], 8_000);
        assert_eq!(value["segments"].as_array().unwrap().len(), 2);
    }
}
