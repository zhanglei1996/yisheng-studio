use sha2::{Digest, Sha256};

use crate::error::AppError;

const SERVICE: &str = "com.yishengstudio.desktop";

pub fn credential_ref(provider_id: &str) -> String {
    let digest = Sha256::digest(provider_id.as_bytes());
    format!("provider:{}", hex::encode(&digest[..12]))
}

pub fn save(provider_id: &str, secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Err(AppError::Validation("credential cannot be empty".into()));
    }
    let reference = credential_ref(provider_id);
    keyring::Entry::new(SERVICE, &reference)
        .map_err(|error| AppError::Credential(redact(&error.to_string())))?
        .set_password(secret)
        .map_err(|error| AppError::Credential(redact(&error.to_string())))?;
    Ok(reference)
}

pub fn get(reference: &str) -> Result<String, AppError> {
    keyring::Entry::new(SERVICE, reference)
        .map_err(|error| AppError::Credential(redact(&error.to_string())))?
        .get_password()
        .map_err(|error| AppError::Credential(redact(&error.to_string())))
}

pub fn delete(reference: &str) -> Result<(), AppError> {
    keyring::Entry::new(SERVICE, reference)
        .map_err(|error| AppError::Credential(redact(&error.to_string())))?
        .delete_credential()
        .map_err(|error| AppError::Credential(redact(&error.to_string())))
}

pub fn redact(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("api key") || lower.contains("secret") || lower.contains("token") {
        "credential operation failed".into()
    } else {
        message.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_is_stable_without_exposing_provider_id() {
        let reference = credential_ref("personal-openai-account@example.com");
        assert_eq!(
            reference,
            credential_ref("personal-openai-account@example.com")
        );
        assert!(!reference.contains("example.com"));
    }

    #[test]
    fn sensitive_errors_are_redacted() {
        assert_eq!(redact("invalid API key abc"), "credential operation failed");
    }
}
