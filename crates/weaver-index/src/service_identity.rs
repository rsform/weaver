//! Service identity management
//!
//! Handles keypair generation, persistence, and DID document creation
//! for service authentication.

use std::path::Path;

use jacquard::from_json_value;
use jacquard::types::crypto::multikey;
use jacquard::types::did_doc::DidDocument;
use jacquard::types::string::Did;
use k256::ecdsa::SigningKey;
use miette::{IntoDiagnostic, Result, WrapErr};
use serde_json::json;
use tracing::info;

/// Service identity containing the signing keypair
pub struct ServiceIdentity {
    signing_key: SigningKey,
    public_key_multibase: String,
}

impl ServiceIdentity {
    /// Load or generate a service identity keypair
    ///
    /// If the key file exists, loads it. Otherwise generates a new keypair
    /// and saves it to the specified path.
    pub fn load_or_generate(key_path: &Path) -> Result<Self> {
        if key_path.exists() {
            Self::load(key_path)
        } else {
            let identity = Self::generate()?;
            identity.save(key_path)?;
            Ok(identity)
        }
    }

    /// Generate a new random keypair
    pub fn generate() -> Result<Self> {
        info!("generating new service identity keypair");
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let public_key_multibase = Self::encode_public_key(&signing_key);
        Ok(Self {
            signing_key,
            public_key_multibase,
        })
    }

    /// Load keypair from file
    fn load(key_path: &Path) -> Result<Self> {
        info!(?key_path, "loading service identity keypair");
        let key_bytes = std::fs::read(key_path)
            .into_diagnostic()
            .wrap_err("failed to read service key file")?;

        if key_bytes.len() != 32 {
            miette::bail!(
                "invalid key file: expected 32 bytes, got {}",
                key_bytes.len()
            );
        }

        let signing_key = SigningKey::from_slice(&key_bytes)
            .map_err(|e| miette::miette!("invalid key data: {}", e))?;
        let public_key_multibase = Self::encode_public_key(&signing_key);

        Ok(Self {
            signing_key,
            public_key_multibase,
        })
    }

    /// Save keypair to file
    fn save(&self, key_path: &Path) -> Result<()> {
        info!(?key_path, "saving service identity keypair");

        // Ensure parent directory exists
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)
                .into_diagnostic()
                .wrap_err("failed to create key directory")?;
        }

        let key_bytes = self.signing_key.to_bytes();
        std::fs::write(key_path, key_bytes.to_vec())
            .into_diagnostic()
            .wrap_err("failed to write service key file")?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(key_path, perms)
                .into_diagnostic()
                .wrap_err("failed to set key file permissions")?;
        }

        Ok(())
    }

    /// Encode the public key as a multikey string
    fn encode_public_key(signing_key: &SigningKey) -> String {
        let verifying_key = signing_key.verifying_key();
        let point = verifying_key.to_encoded_point(true); // compressed
        let bytes = point.as_bytes();
        // 0xE7 is the multicodec for secp256k1-pub
        multikey(0xE7, bytes)
    }

    /// Get the public key multibase string
    pub fn public_key_multibase(&self) -> &str {
        &self.public_key_multibase
    }

    /// Get the signing key (for signing JWTs)
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Build a DID document for this service identity
    pub fn did_document(&self, service_did: &Did<'_>) -> DidDocument<'static> {
        let did_str = service_did.as_str();

        let doc = json!({
            "@context": [
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/multikey/v1",
                "https://w3id.org/security/suites/secp256k1-2019/v1"
            ],
            "id": did_str,
            "verificationMethod": [{
                "id": format!("{}#atproto", did_str),
                "type": "Multikey",
                "controller": did_str,
                "publicKeyMultibase": self.public_key_multibase
            }]
        });

        from_json_value::<DidDocument>(doc).expect("valid DID document")
    }

    /// Build a DID document with a service endpoint
    pub fn did_document_with_service(
        &self,
        service_did: &Did<'_>,
        service_endpoint: &str,
    ) -> DidDocument<'static> {
        let did_str = service_did.as_str();

        let doc = json!({
            "@context": [
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/multikey/v1",
                "https://w3id.org/security/suites/secp256k1-2019/v1"
            ],
            "id": did_str,
            "verificationMethod": [{
                "id": format!("{}#atproto", did_str),
                "type": "Multikey",
                "controller": did_str,
                "publicKeyMultibase": self.public_key_multibase
            }],
            "service": [{
                "id": "#atproto_index",
                "type": "AtprotoRecordIndex",
                "serviceEndpoint": service_endpoint
            }]
        });

        from_json_value::<DidDocument>(doc).expect("valid DID document")
    }
}
