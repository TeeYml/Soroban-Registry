// Contract verification engine
// Compiles source code and compares with on-chain bytecode

use shared::RegistryError;
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;
use sha2::{Sha256, Digest};
use tempfile::tempdir;

pub struct VerificationOutput {
    pub is_verified: bool,
    pub compiler_output: String,
    pub built_wasm_hash: Option<String>,
}
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shared::RegistryError;
use std::{fs, process::Stdio, time::Duration};
use tempfile::TempDir;
use tokio::{process::Command, time::timeout};

const DEFAULT_SOROBAN_SDK_VERSION: &str = "21.7.7";
const BUILD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub verified: bool,
    pub compiled_wasm_hash: String,
    pub deployed_wasm_hash: String,
    pub message: Option<String>,
}

/// Verify that source code matches deployed contract bytecode.
pub async fn verify_contract(
    source_url: &str,
    compiler_version: &str,
    build_params: &serde_json::Value,
    deployed_wasm_hash: &str,
) -> Result<VerificationOutput, RegistryError> {
    // 5 minutes timeout per verification request
    let timeout_duration = Duration::from_secs(300);

    let result = timeout(
        timeout_duration,
        run_verification(source_url, compiler_version, build_params, deployed_wasm_hash),
    )
    .await;

    match result {
        Ok(res) => res,
        Err(_) => Err(RegistryError::VerificationFailed(
            "Verification timed out after 5 minutes".to_string(),
        )),
    }
}

async fn run_verification(
    source_url: &str,
    _compiler_version: &str,
    _build_params: &serde_json::Value,
    deployed_wasm_hash: &str,
) -> Result<VerificationOutput, RegistryError> {
    tracing::info!("Verification requested for contract with hash: {}", deployed_wasm_hash);

    let dir = tempdir().map_err(|e| RegistryError::Internal(format!("Failed to create temp dir: {}", e)))?;
    let repo_path = dir.path().join("repo");

    // 1. Clone Git repository
    if source_url.starts_with("http") || source_url.starts_with("git") {
        let output = Command::new("git")
            .arg("clone")
            .arg(source_url)
            .arg(&repo_path)
            .output()
            .await
            .map_err(|e| RegistryError::Internal(format!("Failed to execute git clone: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(RegistryError::VerificationFailed(format!("Failed to clone repository: {}", err)));
        }
    } else {
        return Err(RegistryError::InvalidInput("Only Git URLs are currently supported for source_code".to_string()));
    }

    // 2. Compile using standard cargo build for WASM target
    let build_output = Command::new("cargo")
        .arg("build")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .arg("--release")
        .current_dir(&repo_path)
        .output()
        .await
        .map_err(|e| RegistryError::Internal(format!("Failed to execute cargo build: {}", e)))?;

    let mut compiler_output = String::from_utf8_lossy(&build_output.stdout).into_owned();
    compiler_output.push_str("\n");
    compiler_output.push_str(&String::from_utf8_lossy(&build_output.stderr));

    if !build_output.status.success() {
        return Ok(VerificationOutput {
            is_verified: false,
            compiler_output,
            built_wasm_hash: None,
        });
    }

    // 3. Find the compiled WASM in target/wasm32-unknown-unknown/release
    let release_dir = repo_path.join("target").join("wasm32-unknown-unknown").join("release");
    let mut wasm_file = None;
    if release_dir.exists() {
        let mut entries = fs::read_dir(release_dir).await.map_err(|e| RegistryError::Internal(e.to_string()))?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| RegistryError::Internal(e.to_string()))? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                wasm_file = Some(path);
                break;
            }
        }
    }

    let wasm_file = match wasm_file {
        Some(f) => f,
        None => {
            return Ok(VerificationOutput {
                is_verified: false,
                compiler_output: compiler_output + "\nError: No WASM file found after build.",
                built_wasm_hash: None,
            });
        }
    };

    // 4. Hash the resulting WASM file (SHA256)
    let wasm_bytes = fs::read(&wasm_file)
        .await
        .map_err(|e| RegistryError::Internal(format!("Failed to read WASM: {}", e)))?;
    
    let mut hasher = Sha256::new();
    hasher.update(&wasm_bytes);
    let built_hash = hex::encode(hasher.finalize());

    // 5. Compare generated WASM with on-chain bytecode
    let is_verified = built_hash.eq_ignore_ascii_case(deployed_wasm_hash);

    Ok(VerificationOutput {
        is_verified,
        compiler_output,
        built_wasm_hash: Some(built_hash),
    })
}

/// Helper just to expose compilation logic directly if needed
pub async fn compile_contract(_source_code: &str) -> Result<Vec<u8>, RegistryError> {
    Err(RegistryError::Internal("Raw compilation not fully supported, use verify_contract directly".to_string()))
    source_code: &str,
    deployed_wasm_hash: &str,
    compiler_version: Option<&str>,
    build_params: Option<&Value>,
) -> Result<VerificationResult, RegistryError> {
    if source_code.trim().is_empty() {
        return Err(RegistryError::InvalidInput(
            "source_code cannot be empty".to_string(),
        ));
    }

    let deployed_normalized = normalize_hash(deployed_wasm_hash).ok_or_else(|| {
        RegistryError::InvalidInput("deployed_wasm_hash must be a 64-char hex hash".to_string())
    })?;

    tracing::info!(
        deployed_wasm_hash = %deployed_normalized,
        "Starting contract verification"
    );

    let compiled_wasm = compile_contract(source_code, compiler_version, build_params).await?;
    let compiled_hash = hash_wasm(&compiled_wasm);

    if compiled_hash == deployed_normalized {
        return Ok(VerificationResult {
            verified: true,
            compiled_wasm_hash: compiled_hash,
            deployed_wasm_hash: deployed_normalized,
            message: None,
        });
    }

    Ok(VerificationResult {
        verified: false,
        compiled_wasm_hash: compiled_hash.clone(),
        deployed_wasm_hash: deployed_normalized.clone(),
        message: Some(format!(
            "Bytecode mismatch: compiled hash {} does not match deployed hash {}",
            compiled_hash, deployed_normalized
        )),
    })
}

/// Compile Rust source code to WASM.
/// Supports two source modes:
/// - raw Rust contract source (compiled with cargo)
/// - `wasm_base64:<...>` for precompiled test payloads
pub async fn compile_contract(
    source_code: &str,
    compiler_version: Option<&str>,
    build_params: Option<&Value>,
) -> Result<Vec<u8>, RegistryError> {
    if let Some(encoded) = source_code.trim().strip_prefix("wasm_base64:") {
        return BASE64.decode(encoded.trim()).map_err(|e| {
            RegistryError::InvalidInput(format!("Invalid wasm_base64 payload: {}", e))
        });
    }

    let temp_dir = TempDir::new()?;
    bootstrap_project(temp_dir.path(), source_code, compiler_version)?;

    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(temp_dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(params) = build_params {
        apply_build_params(&mut command, params);
    }

    let output = timeout(BUILD_TIMEOUT, command.output())
        .await
        .map_err(|_| RegistryError::VerificationFailed("Compilation timed out".to_string()))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = format!(
            "Compilation failed. stdout: {} stderr: {}",
            truncate_for_error(&stdout),
            truncate_for_error(&stderr)
        );
        return Err(RegistryError::VerificationFailed(details));
    }

    let wasm_path = temp_dir
        .path()
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("verify_contract.wasm");

    // Reading the compiled wasm artifact; io errors convert via `From` implementation
    Ok(fs::read(&wasm_path)?)
}

fn bootstrap_project(
    root: &std::path::Path,
    source_code: &str,
    compiler_version: Option<&str>,
) -> Result<(), RegistryError> {
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir)?;

    let sdk_version = compiler_version
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(DEFAULT_SOROBAN_SDK_VERSION);
    let cargo_toml = format!(
        "[package]\nname = \"verify_contract\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nsoroban-sdk = \"{}\"\n",
        sdk_version
    );

    let cargo_path = root.join("Cargo.toml");
    fs::write(&cargo_path, cargo_toml)?;

    let lib_path = src_dir.join("lib.rs");
    fs::write(&lib_path, source_code)?;

    Ok(())
}

fn apply_build_params(command: &mut Command, build_params: &Value) {
    if let Some(profile) = build_params.get("profile").and_then(Value::as_str) {
        command.arg("--profile").arg(profile);
    }
    if let Some(features) = build_params.get("features").and_then(Value::as_array) {
        let joined = features
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",");
        if !joined.is_empty() {
            command.arg("--features").arg(joined);
        }
    }
}

pub fn hash_wasm(wasm_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(wasm_bytes);
    hex::encode(hasher.finalize())
}

pub fn normalize_hash(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let stripped = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if stripped.len() != 64 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(stripped.to_ascii_lowercase())
}

fn truncate_for_error(value: &str) -> String {
    const MAX_ERROR_LEN: usize = 1_000;
    if value.len() <= MAX_ERROR_LEN {
        return value.to_string();
    }
    let mut out = value[..MAX_ERROR_LEN].to_string();
    out.push_str("...[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_verify_contract_invalid_url() {
        let result = verify_contract(
            "invalid_url", 
            "1.0.0", 
            &serde_json::Value::Null, 
            "some_hash"
        ).await;
        
        assert!(result.is_err());
        if let Err(RegistryError::InvalidInput(msg)) = result {
            assert!(msg.contains("Only Git URLs are currently supported"));
        } else {
            panic!("Expected InvalidInput error");
        }
    async fn verify_contract_matches_known_good_wasm_pair() {
        let wasm = b"known-good-wasm";
        let expected_hash = hash_wasm(wasm);
        let source = format!("wasm_base64:{}", BASE64.encode(wasm));

        let result = verify_contract(&source, &expected_hash, None, None)
            .await
            .expect("verification should succeed");

        assert!(result.verified);
        assert_eq!(result.compiled_wasm_hash, expected_hash);
        assert!(result.message.is_none());
    }

    #[tokio::test]
    async fn verify_contract_detects_mismatch_for_known_bad_pair() {
        let source = format!("wasm_base64:{}", BASE64.encode(b"known-bad-wasm"));
        let wrong_hash = hash_wasm(b"different-wasm");

        let result = verify_contract(&source, &wrong_hash, None, None)
            .await
            .expect("verification should complete");

        assert!(!result.verified);
        assert!(result
            .message
            .unwrap_or_default()
            .contains("Bytecode mismatch"));
    }
}

