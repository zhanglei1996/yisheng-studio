use std::{
    collections::BTreeMap,
    fmt,
    future::Future,
    pin::Pin,
    process::Command,
    time::{Duration, SystemTime},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

use crate::error::AppError;

pub const ALIYUN_QWEN_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";
pub const ALIYUN_COSYVOICE_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/tts/SpeechSynthesizer";
pub const IFLYTEK_SUPER_TTS_ENDPOINT: &str =
    "wss://cbm01.cn-huabei-1.xf-yun.com/v1/private/mcd9m97e6";

const IFLYTEK_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IFLYTEK_SESSION_TIMEOUT: Duration = Duration::from_secs(90);
const IFLYTEK_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(20);
const ALIYUN_REALTIME_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const ALIYUN_REALTIME_SESSION_TIMEOUT: Duration = Duration::from_secs(240);
const ALIYUN_REALTIME_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(30);

pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SynthesizedAudio, AppError>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioEncoding {
    Aiff,
    Wav,
    Mp3,
    PcmS16Le,
}

#[derive(Debug, Clone)]
pub struct SynthesisRequest {
    pub text: String,
    pub voice_id: String,
    pub style: String,
    pub instructions: Option<String>,
    pub speed: f32,
    pub pitch: f32,
    pub volume: f32,
    pub sample_rate: u32,
    pub target_duration_ms: Option<i64>,
}

impl SynthesisRequest {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.text.trim().is_empty() {
            return Err(AppError::Validation("配音文本不能为空".into()));
        }
        if self.voice_id.trim().is_empty() {
            return Err(AppError::Validation("配音音色不能为空".into()));
        }
        if !(0.5..=2.0).contains(&self.speed)
            || !(0.5..=2.0).contains(&self.pitch)
            || !(0.0..=2.0).contains(&self.volume)
        {
            return Err(AppError::Validation("语速、音调或音量超出范围".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SynthesizedAudio {
    pub bytes: Vec<u8>,
    pub encoding: AudioEncoding,
    pub sample_rate: u32,
    pub request_id: Option<String>,
    pub billed_characters: Option<u64>,
}

pub trait TtsProviderAdapter: Send + Sync {
    fn driver(&self) -> &'static str;
    fn synthesize<'a>(
        &'a self,
        request: &'a SynthesisRequest,
        secret: &'a TtsSecretBundle,
    ) -> ProviderFuture<'a>;
}

/// Keychain stores the serialized bundle as one password entry. This type is
/// intentionally Deserialize-only and redacts Debug output so a future log or
/// error cannot accidentally expose credentials.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsSecretBundle {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    api_secret: Option<String>,
    #[serde(default)]
    api_password: Option<String>,
}

impl TtsSecretBundle {
    pub fn local() -> Self {
        Self::default()
    }
}

/// Runs the smallest real synthesis request used by the desktop provider
/// connection test. The caller supplies a Keychain reference; secret material
/// never crosses stdout or the command line.
pub async fn smoke_test_keychain_provider(
    driver: &str,
    public_config_json: &str,
    credential_reference: &str,
) -> Result<SynthesizedAudio, AppError> {
    let raw_secret = crate::credentials::get(credential_reference)?;
    let secret = TtsSecretBundle::from_keychain_value(driver, &raw_secret)?;
    let config = serde_json::from_str::<Value>(public_config_json)
        .map_err(|_| AppError::Provider("语音服务配置无法解析".into()))?;
    let voice_id = config
        .get("voice")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(if driver == "iflytek_super_tts" {
            "x6_lingxiaoxuan_flow"
        } else {
            "Cherry"
        });
    let request = SynthesisRequest {
        text: "连接测试。".into(),
        voice_id: voice_id.into(),
        style: "professional".into(),
        instructions: Some("简短、自然地读出连接测试，不新增内容。".into()),
        speed: 1.0,
        pitch: 1.0,
        volume: 1.0,
        sample_rate: 24_000,
        target_duration_ms: None,
    };
    match driver {
        "aliyun_tts" | "bailian_tts" => {
            let model = config
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("qwen3-tts-instruct-flash");
            let region = config
                .get("region")
                .and_then(Value::as_str)
                .unwrap_or("cn-beijing");
            let endpoint = aliyun_endpoint_from_public_config(model, region, &config)?;
            AliyunTtsAdapter::new(AliyunTtsConfig {
                model: model.into(),
                endpoint: Some(endpoint),
                region: region.into(),
                optimize_instructions: false,
                sample_rate: 24_000,
            })?
            .synthesize(&request, &secret)
            .await
        }
        "iflytek_super_tts" | "iflytek" => {
            let endpoint = config
                .get("baseUrl")
                .and_then(Value::as_str)
                .unwrap_or(IFLYTEK_SUPER_TTS_ENDPOINT);
            IflytekSuperTtsAdapter::new(IflytekSuperTtsConfig {
                endpoint: endpoint.into(),
                oral_level: "mid".into(),
                spark_assist: false,
                remain_original: true,
                sample_rate: 24_000,
            })?
            .synthesize(&request, &secret)
            .await
        }
        _ => Err(AppError::Provider("当前语音服务驱动尚未适配".into())),
    }
}

fn aliyun_endpoint_from_public_config(
    model: &str,
    region: &str,
    config: &Value,
) -> Result<String, AppError> {
    let cosyvoice = is_cosyvoice_http_model(model);
    if cosyvoice && region != "cn-beijing" {
        return Err(AppError::Provider(
            "CosyVoice HTTP 合成目前仅支持北京地域".into(),
        ));
    }
    let path = if cosyvoice {
        "/api/v1/services/audio/tts/SpeechSynthesizer"
    } else {
        "/api/v1/services/aigc/multimodal-generation/generation"
    };
    let default_base = match region {
        "cn-beijing" => "https://dashscope.aliyuncs.com/api/v1",
        "ap-southeast-1" => "https://dashscope-intl.aliyuncs.com/api/v1",
        _ => return Err(AppError::Provider("不支持的阿里百炼地域".into())),
    };
    let base = config
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base)
        .trim_end_matches('/');
    let base = base.strip_suffix("/api/v1").unwrap_or(base);
    Ok(format!("{base}{path}"))
}

impl fmt::Debug for TtsSecretBundle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TtsSecretBundle([REDACTED])")
    }
}

impl TtsSecretBundle {
    pub fn from_keychain_value(driver: &str, value: &str) -> Result<Self, AppError> {
        if value.trim().is_empty() {
            return Err(AppError::Credential("credential bundle is empty".into()));
        }
        // Backward compatibility: old profiles stored a single API key.
        if !value.trim_start().starts_with('{') {
            let bundle = Self {
                api_key: Some(value.into()),
                ..Self::default()
            };
            bundle.validate_for(driver)?;
            return Ok(bundle);
        }
        let bundle: Self = serde_json::from_str(value)
            .map_err(|_| AppError::Credential("credential bundle is invalid".into()))?;
        bundle.validate_for(driver)?;
        Ok(bundle)
    }

    pub fn validate_for(&self, driver: &str) -> Result<(), AppError> {
        let valid = match driver {
            "system" => true,
            "aliyun" | "aliyun_tts" | "bailian_tts" => self.api_key_present(),
            "iflytek" | "iflytek_super_tts" => {
                self.app_id_present()
                    && (self.api_password_present()
                        || (self.api_key_present() && self.api_secret_present()))
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(AppError::Credential(
                "credential bundle is incomplete".into(),
            ))
        }
    }

    pub fn validate_public_app_id(&self, expected_app_id: Option<&str>) -> Result<(), AppError> {
        let Some(expected) = expected_app_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(());
        };
        if self.app_id()? == expected {
            Ok(())
        } else {
            Err(AppError::Credential(
                "讯飞 AppID 已变更，请重新输入并保存与该 AppID 匹配的凭据".into(),
            ))
        }
    }

    fn api_key(&self) -> Result<&str, AppError> {
        self.api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::Credential("credential bundle is incomplete".into()))
    }
    fn app_id(&self) -> Result<&str, AppError> {
        self.app_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::Credential("credential bundle is incomplete".into()))
    }
    fn api_secret(&self) -> Result<&str, AppError> {
        self.api_secret
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::Credential("credential bundle is incomplete".into()))
    }
    fn api_key_present(&self) -> bool {
        self.api_key().is_ok()
    }
    fn app_id_present(&self) -> bool {
        self.app_id().is_ok()
    }
    fn api_secret_present(&self) -> bool {
        self.api_secret().is_ok()
    }
    fn api_password_present(&self) -> bool {
        self.api_password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AliyunTtsConfig {
    pub model: String,
    pub endpoint: Option<String>,
    pub region: String,
    pub optimize_instructions: bool,
    pub sample_rate: u32,
}

impl Default for AliyunTtsConfig {
    fn default() -> Self {
        Self {
            model: "qwen3-tts-instruct-flash".into(),
            endpoint: None,
            region: "cn-beijing".into(),
            optimize_instructions: false,
            sample_rate: 24_000,
        }
    }
}

impl AliyunTtsConfig {
    pub fn endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or_else(|| {
            if is_cosyvoice_http_model(&self.model) {
                ALIYUN_COSYVOICE_ENDPOINT
            } else {
                ALIYUN_QWEN_ENDPOINT
            }
        })
    }

    pub fn build_body(&self, request: &SynthesisRequest) -> Result<Value, AppError> {
        request.validate()?;
        if is_qwen_http_model(&self.model) {
            if request.text.chars().count() > 600 {
                return Err(AppError::Validation(
                    "千问语音单次文本不能超过 600 个字符".into(),
                ));
            }
            let mut input = json!({
                "text": request.text,
                "voice": request.voice_id,
                "language_type": "Chinese"
            });
            if is_qwen_instruct_http_model(&self.model) {
                if let Some(instructions) = request
                    .instructions
                    .as_ref()
                    .filter(|v| !v.trim().is_empty())
                {
                    input["instructions"] = json!(instructions);
                    input["optimize_instructions"] = json!(self.optimize_instructions);
                };
            }
            Ok(json!({ "model": self.model, "input": input }))
        } else if is_cosyvoice_http_model(&self.model) {
            // Scheme-three direction is already encoded into `text` as audible
            // punctuation. Do not forward its free-form director prose through
            // CosyVoice's `instruction`: supported voices require strict,
            // voice-specific instruction formats and can reject generic text.
            let input = json!({
                "text": request.text,
                "voice": request.voice_id,
                "format": "wav",
                "sample_rate": self.sample_rate,
                "rate": request.speed,
                "pitch": request.pitch,
                "volume": (request.volume * 50.0).round().clamp(0.0, 100.0) as u8,
                "language_hints": ["zh"]
            });
            Ok(json!({ "model": self.model, "input": input }))
        } else {
            Err(AppError::Provider("当前阿里语音模型尚未适配".into()))
        }
    }
}

pub struct AliyunTtsAdapter {
    config: AliyunTtsConfig,
    client: reqwest::Client,
}

impl AliyunTtsAdapter {
    pub fn new(config: AliyunTtsConfig) -> Result<Self, AppError> {
        let endpoint = Url::parse(config.endpoint())
            .map_err(|_| AppError::Validation("阿里语音服务地址无效".into()))?;
        if !is_allowed_aliyun_tts_endpoint(&endpoint, &config.model) {
            return Err(AppError::Validation(
                "阿里语音服务地址必须是对应模型的百炼北京或新加坡 HTTPS API 地址".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            // Audio is returned as a provider-controlled URL. Do not follow a
            // redirect into localhost/private infrastructure.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if is_allowed_aliyun_audio_url(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .map_err(|_| AppError::Provider("无法初始化阿里语音客户端".into()))?;
        Ok(Self { config, client })
    }

    pub async fn synthesize_realtime_session(
        &self,
        request: &SynthesisRequest,
        text_chunks: &[String],
        secret: &TtsSecretBundle,
    ) -> Result<SynthesizedAudio, AppError> {
        request.validate()?;
        secret.validate_for("aliyun")?;
        if text_chunks.is_empty() || text_chunks.iter().any(|chunk| chunk.trim().is_empty()) {
            return Err(AppError::Validation("连续旁白章节包含空白口播稿".into()));
        }
        let model = aliyun_realtime_model(&self.config.model)?;
        let host = if self
            .config
            .region
            .to_ascii_lowercase()
            .contains("singapore")
            || self.config.endpoint().contains("dashscope-intl")
        {
            "dashscope-intl.aliyuncs.com"
        } else {
            "dashscope.aliyuncs.com"
        };
        let url = format!("wss://{host}/api-ws/v1/realtime?model={model}");
        let mut websocket_request = url
            .into_client_request()
            .map_err(|_| AppError::Provider("阿里实时语音服务地址无效".into()))?;
        websocket_request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", secret.api_key()?))
                .map_err(|_| AppError::Credential("credential bundle is invalid".into()))?,
        );
        websocket_request
            .headers_mut()
            .insert("user-agent", HeaderValue::from_static("yisheng-studio/0.1"));
        let (mut socket, _) = tokio::time::timeout(
            ALIYUN_REALTIME_CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(websocket_request),
        )
        .await
        .map_err(|_| AppError::Provider("阿里实时语音服务连接超时，请检查网络后重试".into()))?
        .map_err(|error| {
            AppError::Provider(format!(
                "阿里实时语音服务连接失败：{}",
                safe_connection_error(&error.to_string())
            ))
        })?;

        let session_id = loop {
            let message = tokio::time::timeout(ALIYUN_REALTIME_FRAME_READ_TIMEOUT, socket.next())
                .await
                .map_err(|_| AppError::Provider("阿里实时语音会话初始化超时".into()))?
                .ok_or_else(|| AppError::Provider("阿里实时语音会话意外关闭".into()))?
                .map_err(|_| AppError::Provider("阿里实时语音会话初始化失败".into()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(text.as_str())
                .map_err(|_| AppError::Provider("阿里实时语音服务返回了无效响应".into()))?;
            if value.get("type").and_then(Value::as_str) == Some("error") {
                return Err(aliyun_realtime_error(&value));
            }
            if value.get("type").and_then(Value::as_str) == Some("session.created") {
                break value
                    .pointer("/session/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        };

        socket
            .send(Message::Text(
                json!({
                    "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                    "type": "session.update",
                    "session": {
                        "voice": request.voice_id,
                        "mode": "server_commit",
                        "language_type": "Chinese",
                        "response_format": "pcm",
                        "sample_rate": request.sample_rate,
                        "instructions": request.instructions.as_deref().unwrap_or(""),
                        "optimize_instructions": self.config.optimize_instructions
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .map_err(|_| AppError::Provider("阿里实时语音配置发送失败".into()))?;
        loop {
            let message = tokio::time::timeout(ALIYUN_REALTIME_FRAME_READ_TIMEOUT, socket.next())
                .await
                .map_err(|_| AppError::Provider("阿里实时语音配置确认超时".into()))?
                .ok_or_else(|| AppError::Provider("阿里实时语音会话意外关闭".into()))?
                .map_err(|_| AppError::Provider("阿里实时语音配置确认失败".into()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(text.as_str())
                .map_err(|_| AppError::Provider("阿里实时语音服务返回了无效响应".into()))?;
            if value.get("type").and_then(Value::as_str) == Some("error") {
                return Err(aliyun_realtime_error(&value));
            }
            if value.get("type").and_then(Value::as_str) == Some("session.updated") {
                break;
            }
        }

        let (mut sender, mut receiver) = socket.split();
        let send_events = async {
            for chunk in text_chunks {
                sender
                    .send(Message::Text(
                        json!({
                            "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                            "type": "input_text_buffer.append",
                            "text": chunk
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .map_err(|_| AppError::Provider("阿里实时语音正文发送失败".into()))?;
                // Feed the session like a natural text stream. This gives
                // ServerCommit enough context without forcing sentence resets.
                tokio::time::sleep(Duration::from_millis(45)).await;
            }
            sender
                .send(Message::Text(
                    json!({
                        "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                        "type": "session.finish"
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .map_err(|_| AppError::Provider("阿里实时语音结束事件发送失败".into()))?;
            Ok::<(), AppError>(())
        };
        let receive_audio = async {
            let mut bytes = Vec::new();
            loop {
                let message =
                    tokio::time::timeout(ALIYUN_REALTIME_FRAME_READ_TIMEOUT, receiver.next())
                        .await
                        .map_err(|_| AppError::Provider("阿里实时语音音频流读取超时".into()))?
                        .ok_or_else(|| AppError::Provider("阿里实时语音音频流提前结束".into()))?
                        .map_err(|_| AppError::Provider("阿里实时语音音频流读取失败".into()))?;
                let Message::Text(text) = message else {
                    continue;
                };
                let value: Value = serde_json::from_str(text.as_str())
                    .map_err(|_| AppError::Provider("阿里实时语音服务返回了无效响应".into()))?;
                if ingest_aliyun_realtime_event(&value, &mut bytes)? {
                    return Ok::<Vec<u8>, AppError>(bytes);
                }
            }
        };
        let (send_result, receive_result) = tokio::time::timeout(
            ALIYUN_REALTIME_SESSION_TIMEOUT,
            futures_util::future::join(send_events, receive_audio),
        )
        .await
        .map_err(|_| AppError::Provider("阿里实时语音章节合成超时，请重试当前章节".into()))?;
        send_result?;
        let bytes = receive_result?;
        if bytes.is_empty() {
            return Err(AppError::Provider(provider_failure(
                "阿里实时语音服务未返回音频",
                session_id.as_deref(),
            )));
        }
        Ok(SynthesizedAudio {
            bytes,
            encoding: AudioEncoding::PcmS16Le,
            sample_rate: request.sample_rate,
            request_id: session_id,
            billed_characters: Some(request.text.chars().count() as u64),
        })
    }

    /// Synthesizes several aligned beats in one Realtime session. Commit mode
    /// keeps the voice/session context alive while response.done gives us an
    /// exact PCM boundary for every beat that can be fitted to a visual anchor.
    pub async fn synthesize_realtime_beats(
        &self,
        request: &SynthesisRequest,
        beats: &[String],
        secret: &TtsSecretBundle,
    ) -> Result<Vec<SynthesizedAudio>, AppError> {
        request.validate()?;
        secret.validate_for("aliyun")?;
        if beats.is_empty() || beats.iter().any(|beat| beat.trim().is_empty()) {
            return Err(AppError::Validation("语义旁白场景包含空白节拍".into()));
        }
        let model = aliyun_realtime_model(&self.config.model)?;
        let host = if self
            .config
            .region
            .to_ascii_lowercase()
            .contains("singapore")
            || self.config.endpoint().contains("dashscope-intl")
        {
            "dashscope-intl.aliyuncs.com"
        } else {
            "dashscope.aliyuncs.com"
        };
        let url = format!("wss://{host}/api-ws/v1/realtime?model={model}");
        let mut websocket_request = url
            .into_client_request()
            .map_err(|_| AppError::Provider("阿里实时语音服务地址无效".into()))?;
        websocket_request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", secret.api_key()?))
                .map_err(|_| AppError::Credential("credential bundle is invalid".into()))?,
        );
        websocket_request
            .headers_mut()
            .insert("user-agent", HeaderValue::from_static("yisheng-studio/0.1"));
        let (mut socket, _) = tokio::time::timeout(
            ALIYUN_REALTIME_CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(websocket_request),
        )
        .await
        .map_err(|_| AppError::Provider("阿里实时语音服务连接超时，请检查网络后重试".into()))?
        .map_err(|error| {
            AppError::Provider(format!(
                "阿里实时语音服务连接失败：{}",
                safe_connection_error(&error.to_string())
            ))
        })?;

        let session_id = loop {
            let message = tokio::time::timeout(ALIYUN_REALTIME_FRAME_READ_TIMEOUT, socket.next())
                .await
                .map_err(|_| AppError::Provider("阿里实时语音会话初始化超时".into()))?
                .ok_or_else(|| AppError::Provider("阿里实时语音会话意外关闭".into()))?
                .map_err(|_| AppError::Provider("阿里实时语音会话初始化失败".into()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(text.as_str())
                .map_err(|_| AppError::Provider("阿里实时语音服务返回了无效响应".into()))?;
            if value.get("type").and_then(Value::as_str) == Some("error") {
                return Err(aliyun_realtime_error(&value));
            }
            if value.get("type").and_then(Value::as_str) == Some("session.created") {
                break value
                    .pointer("/session/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        };
        socket
            .send(Message::Text(
                json!({
                    "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                    "type": "session.update",
                    "session": {
                        "voice": request.voice_id,
                        "mode": "commit",
                        "language_type": "Chinese",
                        "response_format": "pcm",
                        "sample_rate": request.sample_rate,
                        "instructions": request.instructions.as_deref().unwrap_or(""),
                        "optimize_instructions": self.config.optimize_instructions
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .map_err(|_| AppError::Provider("阿里实时语音配置发送失败".into()))?;
        loop {
            let message = tokio::time::timeout(ALIYUN_REALTIME_FRAME_READ_TIMEOUT, socket.next())
                .await
                .map_err(|_| AppError::Provider("阿里实时语音配置确认超时".into()))?
                .ok_or_else(|| AppError::Provider("阿里实时语音会话意外关闭".into()))?
                .map_err(|_| AppError::Provider("阿里实时语音配置确认失败".into()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(text.as_str())
                .map_err(|_| AppError::Provider("阿里实时语音服务返回了无效响应".into()))?;
            if value.get("type").and_then(Value::as_str) == Some("error") {
                return Err(aliyun_realtime_error(&value));
            }
            if value.get("type").and_then(Value::as_str) == Some("session.updated") {
                break;
            }
        }

        let mut outputs = Vec::with_capacity(beats.len());
        for beat in beats {
            socket
                .send(Message::Text(
                    json!({
                        "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                        "type": "input_text_buffer.append",
                        "text": beat
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .map_err(|_| AppError::Provider("阿里实时语音节拍正文发送失败".into()))?;
            socket
                .send(Message::Text(
                    json!({
                        "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                        "type": "input_text_buffer.commit"
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .map_err(|_| AppError::Provider("阿里实时语音节拍提交失败".into()))?;
            let beat_bytes = tokio::time::timeout(ALIYUN_REALTIME_SESSION_TIMEOUT, async {
                let mut bytes = Vec::new();
                loop {
                    let message =
                        tokio::time::timeout(ALIYUN_REALTIME_FRAME_READ_TIMEOUT, socket.next())
                            .await
                            .map_err(|_| AppError::Provider("阿里实时语音节拍读取超时".into()))?
                            .ok_or_else(|| AppError::Provider("阿里实时语音会话提前结束".into()))?
                            .map_err(|_| AppError::Provider("阿里实时语音节拍读取失败".into()))?;
                    let Message::Text(text) = message else {
                        continue;
                    };
                    let value: Value = serde_json::from_str(text.as_str())
                        .map_err(|_| AppError::Provider("阿里实时语音服务返回了无效响应".into()))?;
                    match value.get("type").and_then(Value::as_str) {
                        Some("response.audio.delta") => {
                            let delta =
                                value.get("delta").and_then(Value::as_str).ok_or_else(|| {
                                    AppError::Provider("阿里实时语音服务返回了无效音频分片".into())
                                })?;
                            bytes.extend_from_slice(&BASE64.decode(delta).map_err(|_| {
                                AppError::Provider("阿里实时语音服务返回了损坏的音频分片".into())
                            })?);
                        }
                        Some("response.done") => break Ok::<Vec<u8>, AppError>(bytes),
                        Some("error") => break Err(aliyun_realtime_error(&value)),
                        _ => {}
                    }
                }
            })
            .await
            .map_err(|_| AppError::Provider("阿里实时语音节拍合成超时".into()))??;
            if beat_bytes.is_empty() {
                return Err(AppError::Provider(provider_failure(
                    "阿里实时语音服务未返回节拍音频",
                    session_id.as_deref(),
                )));
            }
            outputs.push(SynthesizedAudio {
                bytes: beat_bytes,
                encoding: AudioEncoding::PcmS16Le,
                sample_rate: request.sample_rate,
                request_id: session_id.clone(),
                billed_characters: Some(beat.chars().count() as u64),
            });
        }
        socket
            .send(Message::Text(
                json!({
                    "event_id": format!("event_{}", uuid::Uuid::new_v4()),
                    "type": "session.finish"
                })
                .to_string()
                .into(),
            ))
            .await
            .map_err(|_| AppError::Provider("阿里实时语音结束事件发送失败".into()))?;
        Ok(outputs)
    }

    async fn synthesize_inner(
        &self,
        request: &SynthesisRequest,
        secret: &TtsSecretBundle,
    ) -> Result<SynthesizedAudio, AppError> {
        secret.validate_for("aliyun")?;
        let response = self
            .client
            .post(self.config.endpoint())
            .bearer_auth(secret.api_key()?)
            .json(&self.config.build_body(request)?)
            .send()
            .await
            .map_err(|error| {
                AppError::Provider(if error.is_timeout() {
                    "阿里语音服务连接超时，请检查网络后重试".into()
                } else {
                    "阿里语音服务连接失败，请检查网络或地域配置".into()
                })
            })?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|_| AppError::Provider("阿里语音服务返回了无效响应".into()))?;
        let request_id = body
            .get("request_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if !is_successful_aliyun_tts_response(status, &body) {
            return Err(AppError::Provider(provider_failure(
                "阿里语音合成失败",
                request_id.as_deref(),
            )));
        }
        let audio_url = body
            .pointer("/output/audio/url")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::Provider(provider_failure(
                    "阿里语音服务未返回音频",
                    request_id.as_deref(),
                ))
            })?;
        let parsed = Url::parse(audio_url)
            .map_err(|_| AppError::Provider("阿里语音服务返回了无效音频地址".into()))?;
        if !is_allowed_aliyun_audio_url(&parsed) {
            return Err(AppError::Provider(
                "阿里语音服务返回了不安全的音频地址".into(),
            ));
        }
        let audio_response = self.client.get(parsed).send().await.map_err(|error| {
            AppError::Provider(if error.is_timeout() {
                "阿里语音音频下载超时，请稍后重试".into()
            } else {
                "阿里语音音频下载失败".into()
            })
        })?;
        if audio_response
            .remote_addr()
            .is_some_and(|address| is_private_or_local_ip(address.ip()))
        {
            return Err(AppError::Provider(
                "阿里语音音频下载连接到了不安全的地址".into(),
            ));
        }
        let bytes = audio_response
            .error_for_status()
            .map_err(|_| AppError::Provider("阿里语音音频下载失败".into()))?
            .bytes()
            .await
            .map_err(|_| AppError::Provider("阿里语音音频读取失败".into()))?
            .to_vec();
        if bytes.is_empty() {
            return Err(AppError::Provider("阿里语音服务返回了空音频".into()));
        }
        Ok(SynthesizedAudio {
            bytes,
            encoding: AudioEncoding::Wav,
            sample_rate: self.config.sample_rate,
            request_id,
            billed_characters: body.pointer("/usage/characters").and_then(Value::as_u64),
        })
    }
}

fn aliyun_realtime_model(model: &str) -> Result<&'static str, AppError> {
    match model {
        "qwen3-tts-instruct-flash" | "qwen3-tts-instruct-flash-realtime" => {
            Ok("qwen3-tts-instruct-flash-realtime")
        }
        "qwen3-tts-flash" | "qwen3-tts-flash-realtime" => Ok("qwen3-tts-flash-realtime"),
        _ => Err(AppError::Provider(
            "连续旁白需要阿里百炼 Qwen3-TTS Flash 或 Instruct Flash 模型".into(),
        )),
    }
}

fn aliyun_realtime_error(value: &Value) -> AppError {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or("unknown_error");
    AppError::Provider(format!("阿里实时语音合成失败（{code}）"))
}

fn ingest_aliyun_realtime_event(value: &Value, bytes: &mut Vec<u8>) -> Result<bool, AppError> {
    match value.get("type").and_then(Value::as_str) {
        Some("response.audio.delta") => {
            let delta = value
                .get("delta")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::Provider("阿里实时语音服务返回了无效音频分片".into()))?;
            let chunk = BASE64
                .decode(delta)
                .map_err(|_| AppError::Provider("阿里实时语音服务返回了损坏的音频分片".into()))?;
            bytes.extend_from_slice(&chunk);
            Ok(false)
        }
        Some("session.finished") => Ok(true),
        Some("error") => Err(aliyun_realtime_error(value)),
        _ => Ok(false),
    }
}

const QWEN_HTTP_MODELS: &[&str] = &[
    "qwen3-tts-flash",
    "qwen3-tts-flash-2025-11-27",
    "qwen3-tts-flash-2025-09-18",
    "qwen3-tts-instruct-flash",
    "qwen3-tts-instruct-flash-2026-01-26",
    "qwen3-tts-vc-2026-01-22",
    "qwen3-tts-vd-2026-01-26",
];

const COSYVOICE_HTTP_MODELS: &[&str] = &[
    "cosyvoice-v3.5-plus",
    "cosyvoice-v3.5-flash",
    "cosyvoice-v3-plus",
    "cosyvoice-v3-flash",
    "cosyvoice-v2",
];

fn is_qwen_http_model(model: &str) -> bool {
    QWEN_HTTP_MODELS.contains(&model)
}

fn is_qwen_instruct_http_model(model: &str) -> bool {
    matches!(
        model,
        "qwen3-tts-instruct-flash" | "qwen3-tts-instruct-flash-2026-01-26"
    )
}

fn is_cosyvoice_http_model(model: &str) -> bool {
    COSYVOICE_HTTP_MODELS.contains(&model)
}

fn is_allowed_aliyun_tts_endpoint(url: &Url, model: &str) -> bool {
    if url.scheme() != "https" || url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    let Some(host) = url.host_str().map(|host| host.to_ascii_lowercase()) else {
        return false;
    };
    let official_host = matches!(
        host.as_str(),
        "dashscope.aliyuncs.com" | "dashscope-intl.aliyuncs.com"
    ) || host.ends_with(".cn-beijing.maas.aliyuncs.com")
        || host.ends_with(".ap-southeast-1.maas.aliyuncs.com");
    let expected_path = if is_qwen_http_model(model) {
        "/api/v1/services/aigc/multimodal-generation/generation"
    } else if is_cosyvoice_http_model(model) {
        "/api/v1/services/audio/tts/SpeechSynthesizer"
    } else {
        return false;
    };
    official_host && url.path() == expected_path
}

fn is_successful_aliyun_tts_response(status: reqwest::StatusCode, body: &Value) -> bool {
    // Current non-streaming Qwen/CosyVoice success payloads may omit the
    // legacy top-level status_code entirely; when present it must still be 2xx.
    let status_code_is_success = body.get("status_code").is_none_or(|value| {
        value
            .as_u64()
            .is_some_and(|code| (200..300).contains(&code))
    });
    let code_is_success = body
        .get("code")
        .is_none_or(|value| value.is_null() || value.as_str().is_some_and(str::is_empty));
    let finished = body
        .pointer("/output/finish_reason")
        .is_none_or(|value| value.as_str() == Some("stop"));
    status.is_success() && status_code_is_success && code_is_success && finished
}

fn is_allowed_aliyun_audio_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let normalized = host.to_ascii_lowercase();
    !is_private_or_local_host(&normalized) && is_allowed_aliyun_oss_host(&normalized)
}

fn is_allowed_aliyun_oss_host(host: &str) -> bool {
    let labels = host.split('.').collect::<Vec<_>>();
    if labels.len() < 4 || labels[labels.len() - 2..] != ["aliyuncs", "com"] {
        return false;
    }
    let bucket = labels[0];
    let endpoint = labels[1];
    !bucket.is_empty()
        && (bucket == "dashscope"
            || bucket.starts_with("dashscope-")
            || bucket.starts_with("dashscope-result"))
        && (endpoint == "oss"
            || endpoint.starts_with("oss-")
            || endpoint.starts_with("oss-accelerate"))
}

fn is_private_or_local_host(host: &str) -> bool {
    let normalized = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if normalized == "localhost" || normalized.ends_with(".localhost") {
        return true;
    }
    if let Ok(address) = normalized.parse::<std::net::IpAddr>() {
        return is_private_or_local_ip(address);
    }
    false
}

fn is_private_or_local_ip(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(value) => {
            value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_unspecified()
                || value.is_broadcast()
        }
        std::net::IpAddr::V6(value) => {
            value.is_loopback() || value.is_unspecified() || value.is_unique_local()
        }
    }
}

impl TtsProviderAdapter for AliyunTtsAdapter {
    fn driver(&self) -> &'static str {
        "aliyun_tts"
    }
    fn synthesize<'a>(
        &'a self,
        request: &'a SynthesisRequest,
        secret: &'a TtsSecretBundle,
    ) -> ProviderFuture<'a> {
        Box::pin(self.synthesize_inner(request, secret))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IflytekSuperTtsConfig {
    pub endpoint: String,
    pub oral_level: String,
    pub spark_assist: bool,
    pub remain_original: bool,
    pub sample_rate: u32,
}

impl Default for IflytekSuperTtsConfig {
    fn default() -> Self {
        Self {
            endpoint: IFLYTEK_SUPER_TTS_ENDPOINT.into(),
            oral_level: "mid".into(),
            spark_assist: true,
            remain_original: true,
            sample_rate: 24_000,
        }
    }
}

pub struct IflytekSuperTtsAdapter {
    config: IflytekSuperTtsConfig,
}

impl IflytekSuperTtsAdapter {
    pub fn new(config: IflytekSuperTtsConfig) -> Result<Self, AppError> {
        let endpoint = Url::parse(&config.endpoint)
            .map_err(|_| AppError::Validation("讯飞语音服务地址无效".into()))?;
        if !is_official_iflytek_endpoint(&endpoint) {
            return Err(AppError::Validation(
                "讯飞语音服务地址必须为官方 Super TTS WSS 地址".into(),
            ));
        }
        if !matches!(config.oral_level.as_str(), "low" | "mid" | "high") {
            return Err(AppError::Validation(
                "讯飞口语化等级仅支持 low、mid 或 high".into(),
            ));
        }
        if !matches!(config.sample_rate, 8_000 | 16_000 | 24_000) {
            return Err(AppError::Validation(
                "讯飞音频采样率仅支持 8000、16000 或 24000 Hz".into(),
            ));
        }
        Ok(Self { config })
    }

    pub fn build_request_body(
        &self,
        request: &SynthesisRequest,
        app_id: &str,
    ) -> Result<Value, AppError> {
        request.validate()?;
        let mut body = json!({
            "header": { "app_id": app_id, "status": 2 },
            "parameter": {
                "tts": {
                    "vcn": request.voice_id,
                    "speed": rate_to_iflytek(request.speed),
                    "volume": (request.volume * 50.0).round().clamp(0.0, 100.0) as u8,
                    "pitch": rate_to_iflytek(request.pitch),
                    "bgs": 0, "reg": 0, "rdn": 0, "rhy": 0,
                    "audio": {
                        "encoding": "raw", "sample_rate": self.config.sample_rate,
                        "channels": 1, "bit_depth": 16, "frame_size": 0
                    }
                }
            },
            "payload": {
                "text": {
                    "encoding": "utf8", "compress": "raw", "format": "plain",
                    "status": 2, "seq": 0, "text": BASE64.encode(request.text.as_bytes())
                }
            }
        });
        // 讯飞公开协议中，oral/spark_assist 仅由 x4 发音人支持。
        // x6 等音色带上这组参数会被服务端拒绝，因此按 VCN 能力发送。
        if request.voice_id.starts_with("x4_") {
            body["parameter"]["oral"] = json!({
                "oral_level": self.config.oral_level,
                "spark_assist": i32::from(self.config.spark_assist),
                "stop_split": 0,
                "remain": i32::from(self.config.remain_original)
            });
        }
        Ok(body)
    }

    fn authorized_request(
        &self,
        secret: &TtsSecretBundle,
    ) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, AppError> {
        let mut request = if let Some(password) = secret
            .api_password
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            let mut request = self
                .config
                .endpoint
                .clone()
                .into_client_request()
                .map_err(|_| AppError::Provider("无法创建讯飞语音连接".into()))?;
            request.headers_mut().insert(
                "x-api-key",
                HeaderValue::from_str(password)
                    .map_err(|_| AppError::Credential("credential bundle is invalid".into()))?,
            );
            request
        } else {
            signed_iflytek_url(
                &self.config.endpoint,
                secret.api_key()?,
                secret.api_secret()?,
            )?
            .into_client_request()
            .map_err(|_| AppError::Provider("无法创建讯飞语音连接".into()))?
        };
        // Avoid forwarding credentials in any debug/error path outside this scope.
        request
            .headers_mut()
            .insert("user-agent", HeaderValue::from_static("yisheng-studio/0.1"));
        Ok(request)
    }

    async fn synthesize_inner(
        &self,
        request: &SynthesisRequest,
        secret: &TtsSecretBundle,
    ) -> Result<SynthesizedAudio, AppError> {
        secret.validate_for("iflytek")?;
        let app_id = secret.app_id()?;
        let payload = self.build_request_body(request, app_id)?;
        let (mut socket, _) = tokio::time::timeout(
            IFLYTEK_CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(self.authorized_request(secret)?),
        )
        .await
        .map_err(|_| AppError::Provider("讯飞语音服务连接超时，请检查网络或服务地址后重试".into()))?
        .map_err(|error| {
            AppError::Provider(format!(
                "讯飞语音服务连接失败：{}",
                safe_connection_error(&error.to_string())
            ))
        })?;
        socket
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|_| AppError::Provider("讯飞语音请求发送失败".into()))?;
        let mut chunks = BTreeMap::<u64, Vec<u8>>::new();
        let mut request_id = None;
        let stream_result = tokio::time::timeout(IFLYTEK_SESSION_TIMEOUT, async {
            loop {
                let message = tokio::time::timeout(IFLYTEK_FRAME_READ_TIMEOUT, socket.next())
                    .await
                    .map_err(|_| {
                        AppError::Provider("讯飞语音音频流读取超时，请检查网络后重试".into())
                    })?;
                let Some(message) = message else {
                    return Err(AppError::Provider(iflytek_stream_truncated_error(
                        request_id.as_deref(),
                    )));
                };
                let message = message.map_err(|_| {
                    AppError::Provider("讯飞语音流读取失败，请检查网络后重试".into())
                })?;
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(_) => {
                        return Err(AppError::Provider(iflytek_stream_truncated_error(
                            request_id.as_deref(),
                        )));
                    }
                    _ => continue,
                };
                let value: Value = serde_json::from_str(text.as_str())
                    .map_err(|_| AppError::Provider("讯飞语音服务返回了无效响应".into()))?;
                if ingest_iflytek_response(&value, &mut chunks, &mut request_id)? {
                    return Ok(());
                }
            }
        })
        .await;
        // Do not let a peer that already completed/failed synthesis hold the
        // command open indefinitely during the WebSocket close handshake.
        let _ = tokio::time::timeout(Duration::from_secs(2), socket.close(None)).await;
        match stream_result {
            Ok(result) => result?,
            Err(_) => {
                return Err(AppError::Provider(
                    "讯飞语音合成超时，请缩短文本或检查网络后重试".into(),
                ));
            }
        }
        let bytes = chunks.into_values().flatten().collect::<Vec<_>>();
        if bytes.is_empty() {
            return Err(AppError::Provider(provider_failure(
                "讯飞语音服务未返回音频",
                request_id.as_deref(),
            )));
        }
        Ok(SynthesizedAudio {
            bytes,
            encoding: AudioEncoding::PcmS16Le,
            sample_rate: self.config.sample_rate,
            request_id,
            billed_characters: Some(request.text.chars().count() as u64),
        })
    }
}

impl TtsProviderAdapter for IflytekSuperTtsAdapter {
    fn driver(&self) -> &'static str {
        "iflytek_super_tts"
    }
    fn synthesize<'a>(
        &'a self,
        request: &'a SynthesisRequest,
        secret: &'a TtsSecretBundle,
    ) -> ProviderFuture<'a> {
        Box::pin(self.synthesize_inner(request, secret))
    }
}

fn is_official_iflytek_endpoint(url: &Url) -> bool {
    url.scheme() == "wss"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("cbm01.cn-huabei-1.xf-yun.com"))
        && url.port().is_none()
        && url.path() == "/v1/private/mcd9m97e6"
        && url.query().is_none()
        && url.fragment().is_none()
}

fn ingest_iflytek_response(
    value: &Value,
    chunks: &mut BTreeMap<u64, Vec<u8>>,
    request_id: &mut Option<String>,
) -> Result<bool, AppError> {
    let code = value
        .pointer("/header/code")
        .or_else(|| value.get("code"))
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::Provider("讯飞语音服务返回了缺少错误码的响应".into()))?;
    let sid = value
        .pointer("/header/sid")
        .or_else(|| value.get("sid"))
        .and_then(Value::as_str);
    *request_id = request_id.take().or_else(|| sid.map(str::to_owned));
    if code != 0 {
        let message = value
            .pointer("/header/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str);
        return Err(AppError::Provider(iflytek_provider_failure(
            code,
            message,
            request_id.as_deref(),
        )));
    }
    let Some(audio) = value.pointer("/payload/audio") else {
        return Ok(false);
    };
    let seq = audio
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::Provider("讯飞语音服务返回了缺少音频序号的响应".into()))?;
    let encoded = audio
        .get("audio")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Provider("讯飞语音服务返回了缺少音频数据的响应".into()))?;
    let decoded = BASE64
        .decode(encoded)
        .map_err(|_| AppError::Provider("讯飞语音服务返回了无效音频".into()))?;
    chunks.insert(seq, decoded);
    match audio.get("status").and_then(Value::as_i64) {
        Some(0 | 1) => Ok(false),
        Some(2) => Ok(true),
        _ => Err(AppError::Provider("讯飞语音服务返回了无效音频状态".into())),
    }
}

fn iflytek_provider_failure(code: i64, message: Option<&str>, request_id: Option<&str>) -> String {
    let mut detail = format!("讯飞语音合成失败（错误码：{code}");
    if let Some(safe_message) = message.and_then(sanitize_iflytek_diagnostic) {
        if !safe_message.is_empty() {
            detail.push_str("；服务信息：");
            detail.push_str(&safe_message);
        }
    }
    if let Some(request_id) = request_id.and_then(sanitize_iflytek_diagnostic) {
        detail.push_str("；请求 ID：");
        detail.push_str(&request_id);
    }
    detail.push('）');
    detail
}

fn sanitize_iflytek_diagnostic(value: &str) -> Option<String> {
    let safe = value
        .chars()
        .filter(|character| !character.is_control())
        .take(300)
        .collect::<String>();
    if safe.trim().is_empty() {
        return None;
    }
    let normalized = safe.to_ascii_lowercase();
    if [
        "api_key",
        "api_secret",
        "api_password",
        "authorization",
        "x-api-key",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return Some("服务端返回了敏感诊断信息，已隐藏".into());
    }
    Some(safe)
}

fn iflytek_stream_truncated_error(request_id: Option<&str>) -> String {
    provider_failure("讯飞语音音频流提前结束，未收到末帧", request_id)
}

#[derive(Debug, Default)]
pub struct SystemTtsAdapter;

impl TtsProviderAdapter for SystemTtsAdapter {
    fn driver(&self) -> &'static str {
        "system"
    }
    fn synthesize<'a>(
        &'a self,
        request: &'a SynthesisRequest,
        _secret: &'a TtsSecretBundle,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            request.validate()?;
            if !cfg!(target_os = "macos") {
                return Err(AppError::Provider("系统语音当前仅支持 macOS".into()));
            }
            let target =
                std::env::temp_dir().join(format!("yisheng-system-{}.aiff", uuid::Uuid::new_v4()));
            let rate = (200.0 * request.speed)
                .round()
                .clamp(100.0, 400.0)
                .to_string();
            let output = Command::new("/usr/bin/say")
                .args(["-v", request.voice_id.as_str(), "-r", rate.as_str(), "-o"])
                .arg(&target)
                .arg(&request.text)
                .output()
                .map_err(|_| AppError::Provider("系统语音生成失败".into()))?;
            if !output.status.success() {
                let _ = std::fs::remove_file(&target);
                return Err(AppError::Provider("系统语音生成失败".into()));
            }
            let bytes = std::fs::read(&target)
                .map_err(|_| AppError::Provider("系统语音音频读取失败".into()))?;
            let _ = std::fs::remove_file(target);
            Ok(SynthesizedAudio {
                bytes,
                encoding: AudioEncoding::Aiff,
                sample_rate: request.sample_rate,
                request_id: None,
                billed_characters: None,
            })
        })
    }
}

fn rate_to_iflytek(rate: f32) -> u8 {
    if rate <= 1.0 {
        (((rate.clamp(0.5, 1.0) - 0.5) / 0.5) * 50.0).round() as u8
    } else {
        (50.0 + (rate.clamp(1.0, 2.0) - 1.0) * 50.0).round() as u8
    }
}

fn signed_iflytek_url(endpoint: &str, api_key: &str, api_secret: &str) -> Result<String, AppError> {
    let mut url =
        Url::parse(endpoint).map_err(|_| AppError::Validation("讯飞语音服务地址无效".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::Validation("讯飞语音服务地址无效".into()))?
        .to_owned();
    let date = httpdate::fmt_http_date(SystemTime::now());
    let origin = format!("host: {host}\ndate: {date}\nGET {} HTTP/1.1", url.path());
    let mut mac = Hmac::<Sha256>::new_from_slice(api_secret.as_bytes())
        .map_err(|_| AppError::Credential("credential bundle is invalid".into()))?;
    mac.update(origin.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());
    let authorization = BASE64.encode(format!(
        "api_key=\"{api_key}\", algorithm=\"hmac-sha256\", headers=\"host date request-line\", signature=\"{signature}\""
    ));
    url.query_pairs_mut()
        .append_pair("host", &host)
        .append_pair("date", &date)
        .append_pair("authorization", &authorization);
    Ok(url.into())
}

fn provider_failure(label: &str, request_id: Option<&str>) -> String {
    request_id
        .map(|id| format!("{label}（请求 ID：{id}）"))
        .unwrap_or_else(|| label.into())
}

fn safe_connection_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("authorization=")
        || lower.contains("x-api-key")
        || lower.contains("api_key")
        || lower.contains("signature")
    {
        "请检查网络、凭据与发音人权限".into()
    } else {
        message
            .chars()
            .filter(|value| !value.is_control())
            .take(180)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SynthesisRequest {
        SynthesisRequest {
            text: "先检索相关上下文。".into(),
            voice_id: "Cherry".into(),
            style: "professional".into(),
            instructions: Some("自然强调检索。".into()),
            speed: 1.0,
            pitch: 1.0,
            volume: 1.0,
            sample_rate: 24_000,
            target_duration_ms: Some(2_000),
        }
    }

    #[test]
    fn secret_debug_is_always_redacted() {
        let bundle =
            TtsSecretBundle::from_keychain_value("aliyun", r#"{"apiKey":"sk-secret"}"#).unwrap();
        let debug = format!("{bundle:?}");
        assert!(!debug.contains("sk-secret"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn aliyun_http_models_map_to_supported_realtime_variants() {
        assert_eq!(
            aliyun_realtime_model("qwen3-tts-instruct-flash").unwrap(),
            "qwen3-tts-instruct-flash-realtime"
        );
        assert_eq!(
            aliyun_realtime_model("qwen3-tts-flash").unwrap(),
            "qwen3-tts-flash-realtime"
        );
        assert!(aliyun_realtime_model("cosyvoice-v3-flash").is_err());
    }

    #[test]
    fn aliyun_realtime_audio_delta_is_appended_and_finish_is_terminal() {
        let mut bytes = Vec::new();
        let payload = BASE64.encode([1_u8, 2, 3, 4]);
        assert!(!ingest_aliyun_realtime_event(
            &json!({"type":"response.audio.delta", "delta":payload}),
            &mut bytes,
        )
        .unwrap());
        assert_eq!(bytes, vec![1, 2, 3, 4]);
        assert!(
            ingest_aliyun_realtime_event(&json!({"type":"session.finished"}), &mut bytes,).unwrap()
        );
    }

    #[test]
    fn qwen_instruct_keeps_freeform_instructions_while_cosyvoice_omits_them() {
        let qwen = AliyunTtsConfig::default().build_body(&request()).unwrap();
        assert_eq!(qwen["model"], "qwen3-tts-instruct-flash");
        assert_eq!(qwen["input"]["language_type"], "Chinese");
        assert_eq!(qwen["input"]["instructions"], "自然强调检索。");
        assert_eq!(qwen["input"]["optimize_instructions"], false);

        for (model, voice) in [
            ("cosyvoice-v3-flash", "longanhuan_v3"),
            ("cosyvoice-v3-plus", "longanhuan"),
        ] {
            let cosy = AliyunTtsConfig {
                model: model.into(),
                ..Default::default()
            };
            let mut cosy_request = request();
            cosy_request.voice_id = voice.into();
            let body = cosy.build_body(&cosy_request).unwrap();
            assert_eq!(body["input"]["format"], "wav");
            assert_eq!(body["input"]["language_hints"][0], "zh");
            assert!(body["input"].get("instruction").is_none());
            assert_eq!(cosy.endpoint(), ALIYUN_COSYVOICE_ENDPOINT);
        }
    }

    #[test]
    fn aliyun_models_and_instructions_are_limited_to_official_http_capabilities() {
        let flash = AliyunTtsConfig {
            model: "qwen3-tts-flash".into(),
            ..Default::default()
        }
        .build_body(&request())
        .unwrap();
        assert!(flash["input"].get("instructions").is_none());

        let realtime = AliyunTtsConfig {
            model: "qwen3-tts-flash-realtime".into(),
            ..Default::default()
        };
        assert!(realtime.build_body(&request()).is_err());

        let mut long = request();
        long.text = "字".repeat(601);
        assert!(AliyunTtsConfig::default().build_body(&long).is_err());
    }

    #[test]
    fn aliyun_endpoint_contract_accepts_only_matching_official_https_routes() {
        let qwen_beijing = Url::parse(ALIYUN_QWEN_ENDPOINT).unwrap();
        let qwen_singapore = Url::parse(
            "https://dashscope-intl.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation",
        )
        .unwrap();
        let cosy_workspace = Url::parse(
            "https://workspace.cn-beijing.maas.aliyuncs.com/api/v1/services/audio/tts/SpeechSynthesizer",
        )
        .unwrap();
        assert!(is_allowed_aliyun_tts_endpoint(
            &qwen_beijing,
            "qwen3-tts-instruct-flash"
        ));
        assert!(is_allowed_aliyun_tts_endpoint(
            &qwen_singapore,
            "qwen3-tts-flash"
        ));
        assert!(is_allowed_aliyun_tts_endpoint(
            &cosy_workspace,
            "cosyvoice-v3-flash"
        ));
        assert!(!is_allowed_aliyun_tts_endpoint(
            &Url::parse(
                "https://example.com/api/v1/services/aigc/multimodal-generation/generation"
            )
            .unwrap(),
            "qwen3-tts-flash"
        ));
        assert!(!is_allowed_aliyun_tts_endpoint(
            &Url::parse(
                "https://dashscope.aliyuncs.com/api/v1/services/audio/tts/SpeechSynthesizer"
            )
            .unwrap(),
            "qwen3-tts-flash"
        ));
    }

    #[test]
    fn aliyun_response_contract_requires_http_and_business_success() {
        let successful = json!({
            "status_code": 200,
            "code": "",
            "output": { "finish_reason": "stop" }
        });
        assert!(is_successful_aliyun_tts_response(
            reqwest::StatusCode::OK,
            &successful
        ));
        let current_non_streaming_success = json!({
            "request_id": "request-id",
            "output": {
                "finish_reason": "stop",
                "audio": { "url": "http://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/result.wav" }
            },
            "usage": { "characters": 4 }
        });
        assert!(is_successful_aliyun_tts_response(
            reqwest::StatusCode::OK,
            &current_non_streaming_success
        ));
        for body in [
            json!({"status_code": 400, "code": "InvalidParameter", "output": {"finish_reason": "stop"}}),
            json!({"status_code": 200, "code": "", "output": {"finish_reason": "null"}}),
            json!({"code": "InvalidParameter", "output": {"finish_reason": "stop"}}),
        ] {
            assert!(!is_successful_aliyun_tts_response(
                reqwest::StatusCode::OK,
                &body
            ));
        }
    }

    #[test]
    fn iflytek_request_uses_base64_text_and_raw_audio() {
        let adapter = IflytekSuperTtsAdapter::new(Default::default()).unwrap();
        let body = adapter.build_request_body(&request(), "app-id").unwrap();
        assert_eq!(
            body["payload"]["text"]["text"],
            BASE64.encode("先检索相关上下文。")
        );
        assert_eq!(body["parameter"]["tts"]["audio"]["encoding"], "raw");
        assert_eq!(body["header"]["status"], 2);
        assert!(body["parameter"].get("oral").is_none());
    }

    #[test]
    fn iflytek_only_sends_oral_controls_to_x4_voices() {
        let adapter = IflytekSuperTtsAdapter::new(Default::default()).unwrap();
        let mut x4_request = request();
        x4_request.voice_id = "x4_example".into();
        let body = adapter.build_request_body(&x4_request, "app-id").unwrap();
        assert_eq!(body["parameter"]["oral"]["spark_assist"], 1);
        assert_eq!(body["parameter"]["oral"]["remain"], 1);
    }

    #[test]
    fn iflytek_rejects_non_official_or_malformed_production_config() {
        for endpoint in [
            "ws://cbm01.cn-huabei-1.xf-yun.com/v1/private/mcd9m97e6",
            "wss://example.com/v1/private/mcd9m97e6",
            "wss://cbm01.cn-huabei-1.xf-yun.com/v1/private/mcd9m97e6?debug=1",
            "wss://cbm01.cn-huabei-1.xf-yun.com/v1/private/mcd9m97e6#fragment",
            "wss://cbm01.cn-huabei-1.xf-yun.com/v1/private/medd90fec",
        ] {
            let error = IflytekSuperTtsAdapter::new(IflytekSuperTtsConfig {
                endpoint: endpoint.into(),
                ..Default::default()
            })
            .err()
            .expect("endpoint must be rejected");
            assert!(error.to_string().contains("官方 Super TTS"));
        }
        for (oral_level, sample_rate) in [("invalid", 24_000), ("mid", 44_100)] {
            assert!(IflytekSuperTtsAdapter::new(IflytekSuperTtsConfig {
                oral_level: oral_level.into(),
                sample_rate,
                ..Default::default()
            })
            .is_err());
        }
    }

    #[test]
    fn iflytek_response_contract_reorders_audio_and_requires_terminal_audio_frame() {
        let mut chunks = BTreeMap::new();
        let mut request_id = None;
        for (seq, status, audio) in [(2, 1, "Yw=="), (0, 0, "YQ=="), (1, 2, "Yg==")] {
            let response = json!({
                "header": { "code": 0, "sid": "sid-1", "status": 1 },
                "payload": { "audio": { "seq": seq, "status": status, "audio": audio } }
            });
            let terminal =
                ingest_iflytek_response(&response, &mut chunks, &mut request_id).unwrap();
            assert_eq!(terminal, status == 2);
        }
        assert_eq!(request_id.as_deref(), Some("sid-1"));
        assert_eq!(chunks.into_values().flatten().collect::<Vec<_>>(), b"abc");

        let mut chunks = BTreeMap::new();
        let mut request_id = Some("sid-2".into());
        let non_terminal = json!({
            "header": { "code": 0 },
            "payload": { "audio": { "seq": 0, "status": 1, "audio": "YQ==" } }
        });
        assert!(!ingest_iflytek_response(&non_terminal, &mut chunks, &mut request_id).unwrap());
        assert!(iflytek_stream_truncated_error(request_id.as_deref()).contains("未收到末帧"));
    }

    #[test]
    fn iflytek_provider_error_keeps_safe_code_message_and_sid() {
        let response = json!({
            "header": { "code": 10163, "message": "参数校验失败\n详情", "sid": "sid-3" }
        });
        let error = ingest_iflytek_response(&response, &mut BTreeMap::new(), &mut None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("10163"));
        assert!(error.contains("参数校验失败详情"));
        assert!(error.contains("sid-3"));
        assert!(!error.contains('\n'));

        let sensitive_response = json!({
            "code": 10010,
            "message": "api_secret=must-not-leak",
            "sid": "sid-4"
        });
        let error = ingest_iflytek_response(&sensitive_response, &mut BTreeMap::new(), &mut None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("10010"));
        assert!(error.contains("sid-4"));
        assert!(!error.contains("must-not-leak"));
    }

    #[test]
    fn audio_download_host_filter_rejects_local_and_private_targets() {
        for host in [
            "localhost",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.8",
            "::1",
        ] {
            assert!(is_private_or_local_host(host), "{host} must be blocked");
        }
        assert!(!is_private_or_local_host(
            "dashscope-result.oss-cn-beijing.aliyuncs.com"
        ));
        assert!(is_allowed_aliyun_audio_url(
            &Url::parse("https://dashscope-result.oss-cn-beijing.aliyuncs.com/out.wav").unwrap()
        ));
        assert!(is_allowed_aliyun_audio_url(
            &Url::parse("http://dashscope-result-bj.oss-cn-beijing.aliyuncs.com/out.wav").unwrap()
        ));
        assert!(is_allowed_aliyun_audio_url(
            &Url::parse("http://dashscope-a717.oss-cn-beijing.aliyuncs.com/out.wav").unwrap()
        ));
        assert!(!is_allowed_aliyun_audio_url(
            &Url::parse("https://example.com/out.wav").unwrap()
        ));
        assert!(!is_allowed_aliyun_audio_url(
            &Url::parse("http://oss-cn-beijing.aliyuncs.com/out.wav").unwrap()
        ));
    }

    #[test]
    fn legacy_single_secret_remains_compatible_only_where_valid() {
        assert!(TtsSecretBundle::from_keychain_value("aliyun", "sk-old").is_ok());
        assert!(TtsSecretBundle::from_keychain_value("iflytek", "old-key").is_err());
    }

    #[test]
    fn iflytek_password_bundle_still_requires_app_id() {
        assert!(TtsSecretBundle::from_keychain_value(
            "iflytek",
            r#"{"apiPassword":"password-only"}"#,
        )
        .is_err());
        assert!(TtsSecretBundle::from_keychain_value(
            "iflytek",
            r#"{"appId":"app","apiPassword":"password"}"#,
        )
        .is_ok());
    }
}
