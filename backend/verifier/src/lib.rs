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

/// Verify that source code matches deployed contract bytecode
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
    }
}

