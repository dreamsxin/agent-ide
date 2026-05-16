const SERVICE_NAME: &str = "agent-ide";
const LLM_PREFIX: &str = "llm-profile";

pub fn llm_credential_ref(profile_id: &str) -> String {
    format!("{}:{}", LLM_PREFIX, profile_id)
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
