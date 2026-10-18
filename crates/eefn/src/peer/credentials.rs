//! Recipient-only credential delivery. Decryption is not grant consumption.
use super::{
    scope::{Role, Scope},
    transport::PeerEndpoint,
};
use crate::authentication::{AuthMode, AuthenticationPolicy};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedMaterial {
    pub schema_version: u32,
    pub scope_sha256: String,
    pub role: Role,
    pub recipient_node: String,
    pub grant_id: String,
    pub token: String,
    pub signature: String,
}
impl std::fmt::Debug for SealedMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedMaterial")
            .field("scope_sha256", &self.scope_sha256)
            .field("role", &self.role)
            .field("recipient_node", &self.recipient_node)
            .field("grant_id", &self.grant_id)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedPair {
    pub source: SealedMaterial,
    pub destination: SealedMaterial,
}

/// Observed local identity/pairing, supplied by the future grant-owned runtime,
/// never by model output or an untrusted discovery response.
pub struct Recipient<'a> {
    pub auth_mode: AuthMode,
    pub node_id: &'a str,
    pub control_session_id: &'a str,
    pub runtime_generation: &'a str,
    pub coordinator_runtime_id: &'a str,
    pub coordinator_certificate: &'a str,
    pub credential: &'a str,
}

// Intentionally no Debug or public fields: private keys must not enter reports.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Material {
    pub(super) scope_sha256: String,
    pub(super) role: Role,
    pub(super) root_certificate: String,
    pub(super) certificate: String,
    pub(super) private_key: String,
    pub(super) peer_certificate: String,
    pub(super) wire_key: String,
}
pub(super) fn der(value: &str) -> Result<Vec<u8>> {
    if value.len() > 8192 {
        bail!("Peer TLS material exceeds bounds")
    }
    let value = STANDARD
        .decode(value)
        .map_err(|_| anyhow::anyhow!("Invalid peer TLS encoding"))?;
    if value.is_empty() || value.len() > 4096 {
        bail!("Peer TLS material exceeds bounds")
    }
    Ok(value)
}
pub(super) fn name(hash: &str, role: Role) -> String {
    format!(
        "{}-{}.{}.eef-peer.invalid",
        if role == Role::Source { "src" } else { "dst" },
        &hash[..32],
        &hash[32..]
    )
}
fn crypto(credential: &str) -> Result<crate::NodeCrypto> {
    crate::authentication::validate_credential(credential)?;
    let secret = hex::decode(credential)?;
    let hkdf = Hkdf::<Sha256>::new(Some(b"eef-peer-material-v1"), &secret);
    let mut key = [0u8; 32];
    hkdf.expand(b"recipient-only-node-credential-delivery", &mut key)
        .map_err(|_| anyhow::anyhow!("Peer material key derivation failed"))?;
    Ok(crate::NodeCrypto::new(key)?)
}
fn aad(scope_hash: &str, role: Role, node: &str, grant: &str) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(
        &json!({"protocol":"eef-peer-material-v1","scope_sha256":scope_hash,"role":role,"recipient_node":node,"grant_id":grant}),
    )?)
}
fn signed_bytes(envelope: &SealedMaterial) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(
        &json!({"protocol":"eef-peer-credential-signature-v1","schema_version":envelope.schema_version,"scope_sha256":envelope.scope_sha256,"role":envelope.role,"recipient_node":envelope.recipient_node,"grant_id":envelope.grant_id,"token":envelope.token}),
    )?)
}
fn verify_signature(certificate: &str, envelope: &SealedMaterial) -> Result<()> {
    use x509_parser::prelude::{FromDer, X509Certificate};
    let bytes = der(certificate)?;
    let (rest, cert) = X509Certificate::from_der(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid peer coordinator certificate"))?;
    if !rest.is_empty() || !cert.validity().is_valid() || envelope.signature.len() > 256 {
        bail!("Peer coordinator certificate or signature is not current")
    }
    let signature = STANDARD
        .decode(&envelope.signature)
        .map_err(|_| anyhow::anyhow!("Invalid peer coordinator signature"))?;
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_ASN1,
        cert.public_key().subject_public_key.data.as_ref(),
    )
    .verify(&signed_bytes(envelope)?, &signature)
    .map_err(|_| anyhow::anyhow!("Peer coordinator signature verification failed"))
}
fn seal(
    scope: &Scope,
    material: Material,
    key: &str,
    identity: &crate::tls::CoordinatorIdentity,
) -> Result<SealedMaterial> {
    let party = scope.party(material.role);
    let role = material.role;
    let hash = scope.digest()?;
    let bytes = serde_json::to_vec(&material)?;
    if bytes.len() > 16384 {
        bail!("Peer material exceeds bounds")
    }
    let mut envelope = SealedMaterial {
        schema_version: 1,
        scope_sha256: hash.clone(),
        role,
        recipient_node: party.node_id.clone(),
        grant_id: party.grant_id.clone(),
        token: crypto(key)?.encrypt(&bytes, &aad(&hash, role, &party.node_id, &party.grant_id)?)?,
        signature: String::new(),
    };
    let random = ring::rand::SystemRandom::new();
    let signer = ring::signature::EcdsaKeyPair::from_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
        &der(&identity.private_key)?,
        &random,
    )
    .map_err(|_| {
        anyhow::anyhow!("Peer credentials require a supported P-256 coordinator signing key")
    })?;
    let signature = signer
        .sign(&random, &signed_bytes(&envelope)?)
        .map_err(|_| anyhow::anyhow!("Peer coordinator signing failed"))?;
    envelope.signature = STANDARD.encode(signature.as_ref());
    verify_signature(&identity.certificate, &envelope)?;
    Ok(envelope)
}

/// Library-only preparation. This does NOT check approval, issue/consume grants,
/// persist receipts, register resources, start a listener or authorize a session.
pub fn seal_pair(
    scope: &Scope,
    authentication: &AuthenticationPolicy,
    legacy_key: &str,
    current_runtime: &str,
) -> Result<SealedPair> {
    if !scope.valid_at(crate::protocol::now_ms()) || scope.coordinator_runtime_id != current_runtime
    {
        bail!("Peer scope is not current")
    }
    authentication.validate()?;
    authentication.validate_legacy_key(legacy_key)?;
    let identity = authentication
        .tls
        .as_ref()
        .context("Peer credentials require pinned coordinator TLS")?;
    if identity.fingerprint()? != scope.issuer_id {
        bail!("Peer coordinator identity differs")
    }
    let key = |node: &str| {
        authentication
            .credentials
            .get(node)
            .and_then(Option::as_deref)
            .context("Peer needs its own unrevoked node credential")
    };
    let source_key = key(&scope.source.node_id)?;
    let destination_key = key(&scope.destination.node_id)?;
    let hash = scope.digest()?;
    let (not_before, not_after) = scope.validity()?;
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new())?;
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    ca.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    ca.not_before = not_before;
    ca.not_after = not_after;
    ca.distinguished_name
        .push(rcgen::DnType::CommonName, format!("EEF peer {hash}"));
    let ca_key = rcgen::KeyPair::generate()?;
    let ca_cert = ca.self_signed(&ca_key)?;
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let leaf = |role: Role| -> Result<(String, String)> {
        let mut params = rcgen::CertificateParams::new(vec![name(&hash, role)])?;
        params.not_before = not_before;
        params.not_after = not_after;
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![if role == Role::Source {
            rcgen::ExtendedKeyUsagePurpose::ClientAuth
        } else {
            rcgen::ExtendedKeyUsagePurpose::ServerAuth
        }];
        let key = rcgen::KeyPair::generate()?;
        let cert = params.signed_by(&key, &issuer)?;
        Ok((
            STANDARD.encode(cert.der()),
            STANDARD.encode(key.serialize_der()),
        ))
    };
    let (source_cert, source_private) = leaf(Role::Source)?;
    let (destination_cert, destination_private) = leaf(Role::Destination)?;
    let wire_key = crate::authentication::generate_credential();
    let source = Material {
        scope_sha256: hash.clone(),
        role: Role::Source,
        root_certificate: STANDARD.encode(ca_cert.der()),
        certificate: source_cert.clone(),
        private_key: source_private,
        peer_certificate: destination_cert.clone(),
        wire_key: wire_key.clone(),
    };
    let destination = Material {
        scope_sha256: hash,
        role: Role::Destination,
        root_certificate: STANDARD.encode(ca_cert.der()),
        certificate: destination_cert,
        private_key: destination_private,
        peer_certificate: source_cert,
        wire_key,
    };
    Ok(SealedPair {
        source: seal(scope, source, source_key, identity)?,
        destination: seal(scope, destination, destination_key, identity)?,
    })
}

/// Verify recipient and decrypt only their bundle. A consumed grant and runtime
/// admission are still required before this endpoint can become a product path.
pub fn open(
    scope: &Scope,
    envelope: &SealedMaterial,
    recipient: Recipient<'_>,
) -> Result<PeerEndpoint> {
    let started = std::time::Instant::now();
    let wall = crate::protocol::now_ms();
    if !scope.valid_at(wall)
        || recipient.auth_mode != AuthMode::NodeCredential
        || recipient.coordinator_runtime_id != scope.coordinator_runtime_id
    {
        bail!("Peer scope or recipient is not current")
    }
    crate::tls::client_config(recipient.coordinator_certificate)?;
    if crate::tls::certificate_fingerprint(recipient.coordinator_certificate)? != scope.issuer_id {
        bail!("Peer issuer differs from owner pairing")
    }
    let party = scope.party(envelope.role);
    let hash = scope.digest()?;
    if envelope.schema_version != 1
        || envelope.scope_sha256 != hash
        || envelope.recipient_node != party.node_id
        || envelope.grant_id != party.grant_id
        || recipient.node_id != party.node_id
        || recipient.control_session_id != party.control_session_id
        || recipient.runtime_generation != party.runtime_generation
        || envelope.token.len() > 32768
    {
        bail!("Peer credential recipient or scope differs")
    }
    verify_signature(recipient.coordinator_certificate, envelope)?;
    let bytes = crypto(recipient.credential)?
        .decrypt(
            &envelope.token,
            &aad(&hash, envelope.role, &party.node_id, &party.grant_id)?,
        )
        .map_err(|_| anyhow::anyhow!("Peer credential verification failed"))?;
    if bytes.len() > 16384 {
        bail!("Peer material exceeds bounds")
    }
    let material: Material = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid peer credential material"))?;
    if material.scope_sha256 != hash || material.role != envelope.role {
        bail!("Peer credential material binding differs")
    }
    crate::authentication::validate_credential(&material.wire_key)?;
    PeerEndpoint::new(scope.clone(), material, started, wall)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn even_valid_recipient_aead_cannot_forge_coordinator_issuance() {
        let f = crate::peer::tests::Fixture::new();
        let role = Role::Source;
        let party = f.scope.party(role);
        let hash = f.scope.digest().unwrap();
        let key = f.recipient(role).credential;
        let binding = aad(&hash, role, &party.node_id, &party.grant_id).unwrap();
        let mut material: Material = serde_json::from_slice(
            &crypto(key)
                .unwrap()
                .decrypt(&f.pair.source.token, &binding)
                .unwrap(),
        )
        .unwrap();
        material.wire_key = crate::authentication::generate_credential();
        let mut forged = f.pair.source.clone();
        forged.token = crypto(key)
            .unwrap()
            .encrypt(&serde_json::to_vec(&material).unwrap(), &binding)
            .unwrap();
        assert!(
            crypto(key)
                .unwrap()
                .decrypt(&forged.token, &binding)
                .is_ok(),
            "valid AEAD alone is not coordinator authority"
        );
        assert!(open(&f.scope, &forged, f.recipient(role)).is_err());
        let control = crate::NodeCrypto::new(key).unwrap();
        assert!(
            control.decrypt(&f.pair.source.token, &binding).is_err(),
            "control and material keys must use distinct domains"
        );
        assert!(!format!("{:?}", f.pair.source).contains(&f.pair.source.token));
    }
}
