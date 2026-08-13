use yisheng_studio_lib::tts_provider::{
    AliyunTtsConfig, IflytekSuperTtsAdapter, SynthesisRequest, TtsProviderAdapter, TtsSecretBundle,
    ALIYUN_COSYVOICE_ENDPOINT, ALIYUN_QWEN_ENDPOINT,
};

fn request() -> SynthesisRequest {
    SynthesisRequest {
        text: "RAG 让准确率提升 12%。".into(),
        voice_id: "Cherry".into(),
        style: "professional".into(),
        instructions: Some("专业、自然，严格照稿朗读。".into()),
        speed: 1.0,
        pitch: 1.0,
        volume: 1.0,
        sample_rate: 24_000,
        target_duration_ms: Some(2_000),
    }
}

#[test]
fn public_adapter_contract_exposes_stable_driver_ids() {
    let aliyun = yisheng_studio_lib::tts_provider::AliyunTtsAdapter::new(Default::default())
        .expect("default Aliyun adapter should be valid");
    let iflytek = IflytekSuperTtsAdapter::new(Default::default())
        .expect("default iFlytek adapter should be valid");

    assert_eq!(aliyun.driver(), "aliyun_tts");
    assert_eq!(iflytek.driver(), "iflytek_super_tts");
}

#[test]
fn aliyun_model_selection_keeps_qwen_and_cosyvoice_endpoints_separate() {
    let qwen = AliyunTtsConfig::default();
    let cosy = AliyunTtsConfig {
        model: "cosyvoice-v3-flash".into(),
        ..Default::default()
    };

    assert_eq!(qwen.endpoint(), ALIYUN_QWEN_ENDPOINT);
    assert_eq!(cosy.endpoint(), ALIYUN_COSYVOICE_ENDPOINT);

    let qwen_body = qwen.build_body(&request()).expect("Qwen body");
    let cosy_body = cosy.build_body(&request()).expect("CosyVoice body");
    assert_eq!(
        qwen_body["input"]["instructions"],
        "专业、自然，严格照稿朗读。"
    );
    assert!(cosy_body["input"].get("instruction").is_none());
}

#[test]
fn aliyun_endpoint_overrides_must_be_official_https_routes_for_the_selected_model() {
    let singapore_qwen = AliyunTtsConfig {
        endpoint: Some(
            "https://dashscope-intl.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
                .into(),
        ),
        ..Default::default()
    };
    assert!(yisheng_studio_lib::tts_provider::AliyunTtsAdapter::new(singapore_qwen).is_ok());

    let mismatched = AliyunTtsConfig {
        endpoint: Some(
            "https://dashscope.aliyuncs.com/api/v1/services/audio/tts/SpeechSynthesizer".into(),
        ),
        ..Default::default()
    };
    assert!(yisheng_studio_lib::tts_provider::AliyunTtsAdapter::new(mismatched).is_err());

    let untrusted = AliyunTtsConfig {
        endpoint: Some(
            "https://example.com/api/v1/services/aigc/multimodal-generation/generation".into(),
        ),
        ..Default::default()
    };
    assert!(yisheng_studio_lib::tts_provider::AliyunTtsAdapter::new(untrusted).is_err());
}

#[test]
fn provider_configs_reject_non_tls_override_endpoints() {
    let aliyun = AliyunTtsConfig {
        endpoint: Some("http://127.0.0.1:8080/tts".into()),
        ..Default::default()
    };
    assert!(yisheng_studio_lib::tts_provider::AliyunTtsAdapter::new(aliyun).is_err());

    let iflytek = yisheng_studio_lib::tts_provider::IflytekSuperTtsConfig {
        endpoint: "ws://127.0.0.1:8080/tts".into(),
        ..Default::default()
    };
    assert!(IflytekSuperTtsAdapter::new(iflytek).is_err());
}

#[test]
fn secret_bundle_errors_and_debug_output_never_echo_supplied_secrets() {
    let secret = "sk-should-never-appear";
    let bundle =
        TtsSecretBundle::from_keychain_value("aliyun", &format!(r#"{{"apiKey":"{secret}"}}"#))
            .expect("valid Aliyun secret bundle");
    assert!(!format!("{bundle:?}").contains(secret));

    let malformed = format!(r#"{{"apiKey":"{secret}""#);
    let message = TtsSecretBundle::from_keychain_value("aliyun", &malformed)
        .expect_err("malformed bundle must fail")
        .to_string();
    assert!(!message.contains(secret));
}

#[test]
fn secret_bundle_requires_the_provider_specific_credential_shape() {
    assert!(TtsSecretBundle::from_keychain_value("aliyun", "legacy-api-key").is_ok());

    let password_auth = TtsSecretBundle::from_keychain_value(
        "iflytek",
        r#"{"appId":"app","apiPassword":"password"}"#,
    )
    .expect("API password credentials should be accepted");
    assert!(password_auth.validate_for("iflytek").is_ok());

    let signature_auth = TtsSecretBundle::from_keychain_value(
        "iflytek",
        r#"{"appId":"app","apiKey":"key","apiSecret":"secret"}"#,
    )
    .expect("HMAC credentials should be accepted");
    assert!(signature_auth.validate_for("iflytek").is_ok());

    let incomplete =
        TtsSecretBundle::from_keychain_value("iflytek", r#"{"appId":"app","apiKey":"key"}"#)
            .expect_err("incomplete HMAC credentials must be rejected")
            .to_string();
    assert!(!incomplete.contains("app"));
    assert!(!incomplete.contains("key"));
}

#[test]
fn request_validation_rejects_empty_text_and_out_of_range_delivery_values() {
    let mut invalid = request();
    invalid.text.clear();
    assert!(invalid.validate().is_err());

    let mut invalid = request();
    invalid.speed = 2.1;
    assert!(invalid.validate().is_err());

    let mut invalid = request();
    invalid.volume = -0.1;
    assert!(invalid.validate().is_err());
}
