use sha2::{Digest, Sha256};

#[cfg(target_os = "macos")]
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::error::AppError;

const SERVICE: &str = "com.yishengstudio.desktop";

pub fn credential_ref(provider_id: &str) -> String {
    let digest = Sha256::digest(provider_id.as_bytes());
    format!("provider:{}", hex::encode(&digest[..12]))
}

pub fn save(provider_id: &str, secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Err(AppError::Validation("凭据不能为空".into()));
    }
    let reference = credential_ref(provider_id);
    keyring::Entry::new(SERVICE, &reference)
        .map_err(|_| keychain_failure("访问"))?
        .set_password(secret)
        .map_err(|_| keychain_failure("写入"))?;
    Ok(reference)
}

pub fn get(reference: &str) -> Result<String, AppError> {
    #[cfg(target_os = "macos")]
    {
        get_with_security_tool(reference)
    }

    #[cfg(not(target_os = "macos"))]
    let entry = keyring::Entry::new(SERVICE, reference).map_err(|_| keychain_failure("访问"))?;
    #[cfg(not(target_os = "macos"))]
    match entry.get_password() {
        Ok(secret) => Ok(secret),
        Err(keyring::Error::NoEntry) => Err(AppError::Credential(
            "未找到已保存的凭据，请在“服务商”页面重新输入并保存".into(),
        )),
        Err(_) => Err(AppError::Credential(
            "无法从 macOS 钥匙串读取凭据；请确认钥匙串已解锁，在“服务商”页面重新保存，并在系统提示时允许访问".into(),
        )),
    }
}

#[cfg(target_os = "macos")]
fn get_with_security_tool(reference: &str) -> Result<String, AppError> {
    let mut child = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            SERVICE,
            "-a",
            reference,
            "-w",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| keychain_failure("访问"))?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return if status.code() == Some(44) {
                        Err(AppError::Credential(
                            "未找到已保存的凭据，请在“服务商”页面重新输入并保存".into(),
                        ))
                    } else {
                        Err(keychain_failure("读取"))
                    };
                }
                let mut bytes = Vec::new();
                child
                    .stdout
                    .take()
                    .ok_or_else(|| keychain_failure("读取"))?
                    .read_to_end(&mut bytes)
                    .map_err(|_| keychain_failure("读取"))?;
                let secret = String::from_utf8(bytes).map_err(|_| {
                    AppError::Credential("钥匙串中的凭据格式不正确，请重新保存".into())
                })?;
                return Ok(secret.trim_end_matches(['\r', '\n']).to_string());
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::Credential(
                    "读取 macOS 钥匙串超时；请确认登录钥匙串已解锁后重试".into(),
                ));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(keychain_failure("读取"));
            }
        }
    }
}

pub fn delete(reference: &str) -> Result<(), AppError> {
    let entry = keyring::Entry::new(SERVICE, reference).map_err(|_| keychain_failure("访问"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(keychain_failure("删除")),
    }
}

fn keychain_failure(action: &str) -> AppError {
    AppError::Credential(format!(
        "无法{action} macOS 钥匙串；请确认钥匙串已解锁，并在系统提示时允许访问"
    ))
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
}
