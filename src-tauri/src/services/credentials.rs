const SERVICE_NAME: &str = "agent-ide";
const LLM_PREFIX: &str = "llm-profile";
const GIT_PREFIX: &str = "git-remote";

pub fn llm_credential_ref(profile_id: &str) -> String {
    format!("{}:{}", LLM_PREFIX, profile_id)
}

pub fn git_credential_ref(remote_url: &str) -> String {
    format!("{}:{}", GIT_PREFIX, remote_url)
}

pub fn store_secret(credential_ref: &str, secret: &str) -> Result<(), String> {
    if credential_ref.trim().is_empty() {
        return Err("Credential reference is required".to_string());
    }
    keyring::Entry::new(SERVICE_NAME, credential_ref)
        .map_err(|e| format!("Credential store unavailable: {}", e))?
        .set_password(secret)
        .map_err(|e| format!("Failed to store credential: {}", e))
}

pub fn read_secret(credential_ref: &str) -> Result<String, String> {
    if credential_ref.trim().is_empty() {
        return Err("Credential reference is required".to_string());
    }
    keyring::Entry::new(SERVICE_NAME, credential_ref)
        .map_err(|e| format!("Credential store unavailable: {}", e))?
        .get_password()
        .map_err(|e| format!("Credential not found or inaccessible: {}", e))
}

pub fn delete_secret(credential_ref: &str) -> Result<(), String> {
    if credential_ref.trim().is_empty() {
        return Ok(());
    }
    match keyring::Entry::new(SERVICE_NAME, credential_ref) {
        Ok(entry) => entry
            .delete_credential()
            .or_else(|_| Ok::<(), keyring::Error>(()))
            .map_err(|e| format!("Failed to delete credential: {}", e)),
        Err(e) => Err(format!("Credential store unavailable: {}", e)),
    }
}

/// 该引用在 OS 凭据存储里是否真的有可读的密钥。
///
/// 只看 `credential_ref` 是否存在是不够的：引导流程会先设置引用、再尝试写入，
/// 写入失败时引用会指向一个不存在的条目，UI 却以为密钥已保存。
pub fn has_secret(credential_ref: &str) -> bool {
    read_secret(credential_ref).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这条测试验证本机 OS 凭据存储真的能写进去再读出来
    /// （Windows Credential Manager / macOS Keychain / Linux Secret Service）。
    /// 它是 Phase 10.5 要求的密钥存储验证，也是排查"配置了 key 仍然报
    /// Credential not found"时的第一道分界线：这里过了说明问题在调用方。
    #[test]
    fn secret_round_trips_through_os_credential_store() {
        let credential_ref = "agent-ide-selftest:credentials-round-trip";
        let _ = delete_secret(credential_ref);

        let stored = store_secret(credential_ref, "round-trip-value");
        assert!(stored.is_ok(), "store_secret failed: {:?}", stored.err());

        assert!(has_secret(credential_ref));
        assert_eq!(read_secret(credential_ref).unwrap(), "round-trip-value");

        // 覆盖写入必须生效，否则"重新输入 key"永远修不好一个坏条目
        store_secret(credential_ref, "second-value").unwrap();
        assert_eq!(read_secret(credential_ref).unwrap(), "second-value");

        delete_secret(credential_ref).unwrap();
        assert!(!has_secret(credential_ref));
    }

    #[test]
    fn empty_reference_is_rejected() {
        assert!(store_secret("", "value").is_err());
        assert!(read_secret("   ").is_err());
        assert!(!has_secret(""));
        // 删除空引用视为幂等成功，避免清理路径报无关错误
        assert!(delete_secret("").is_ok());
    }
}
