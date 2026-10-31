//! Called only inside an existing consumed AdmittedPeer operation. No separate
//! receipt, permissions, task ownership, sockets, restart or automatic retry.
use super::{
    Failure, PeerEndpoint,
    admission::{Definition, TransferReceipt},
    scope::{Role, Scope},
};
use crate::{NodeEngine, grants::ExecutionGrant};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    payload_bytes: u64,
    payload_sha256: String,
}

pub(super) async fn transfer<S: crate::tls::NodeIo + 'static>(
    engine: &NodeEngine,
    g: &ExecutionGrant,
    endpoint: PeerEndpoint,
    role: Role,
    scope: Scope,
    definition: Definition,
    stream: S,
    execution_deadline: tokio::time::Instant,
) -> Result<Value> {
    let audio = definition
        .audio
        .as_ref()
        .context("Missing approved audio operation")?;
    let maximum_encoded = usize::try_from(definition.payload_bytes.div_ceil(3) * 4 + 1024)?;
    let (manifest, frames) = if role == Role::Source {
        let mut channel = endpoint.connect(stream).await.context(Failure::Channel)?;
        engine.check_grant_policy(g)?;
        let (cap, action, params) = audio.operation(role);
        let output = engine
            .execute_with_python_output_limit(
                cap,
                action,
                params,
                Some(maximum_encoded),
                Some(execution_deadline),
            )
            .await
            .context(Failure::Capture)?;
        let bytes = (|| -> Result<Vec<u8>> {
            let encoded = output["data_base64"]
                .as_str()
                .context("Audio capture returned no bounded WAV")?;
            if encoded.len() > maximum_encoded
                || output["mime_type"] != "audio/wav"
                || output["sample_rate"] != audio.sample_rate
                || output["channels"] != audio.channels
            {
                bail!("Audio capture differs from approved format")
            }
            let bytes = STANDARD
                .decode(encoded)
                .context("Audio capture returned invalid encoding")?;
            audio.canonicalize(bytes)
        })()
        .context(Failure::CaptureFormat)?;
        engine.check_grant_policy(g)?;
        let manifest = Manifest {
            schema_version: 2,
            payload_bytes: bytes.len() as u64,
            payload_sha256: hex::encode(Sha256::digest(&bytes)),
        };
        channel
            .send_data(&serde_json::to_vec(&manifest)?)
            .await
            .context(Failure::Transfer)?;
        channel
            .receive_acknowledgement()
            .await
            .context(Failure::Transfer)?;
        for chunk in bytes.chunks(definition.limits.max_frame_bytes as usize) {
            channel.send_data(chunk).await.context(Failure::Transfer)?;
            channel
                .receive_acknowledgement()
                .await
                .context(Failure::Transfer)?;
        }
        let frames = 1 + manifest
            .payload_bytes
            .div_ceil(u64::from(definition.limits.max_frame_bytes)) as u32;
        (manifest, frames)
    } else {
        let mut channel = endpoint.accept(stream).await.context(Failure::Channel)?;
        let encoded = channel.receive_data().await.context(Failure::Transfer)?;
        if encoded.len() > 512 {
            return Err(Failure::Manifest.into());
        }
        let manifest: Manifest = serde_json::from_slice(&encoded).context(Failure::Manifest)?;
        if manifest.schema_version != 2
            || manifest.payload_bytes < audio.minimum_bytes()
            || manifest.payload_bytes > definition.payload_bytes
            || !crate::request_result::valid_digest(&manifest.payload_sha256)
        {
            return Err(Failure::Manifest.into());
        }
        channel.acknowledge().await.context(Failure::Transfer)?;
        let count = manifest
            .payload_bytes
            .div_ceil(u64::from(definition.limits.max_frame_bytes));
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(manifest.payload_bytes as usize)?;
        for index in 0..count {
            let chunk = channel.receive_data().await.context(Failure::Transfer)?;
            let expected = (manifest.payload_bytes - bytes.len() as u64)
                .min(u64::from(definition.limits.max_frame_bytes));
            if chunk.len() as u64 != expected {
                return Err(Failure::Payload.into());
            }
            bytes.extend_from_slice(&chunk);
            if index + 1 < count {
                channel.acknowledge().await.context(Failure::Transfer)?;
            }
        }
        if hex::encode(Sha256::digest(&bytes)) != manifest.payload_sha256 {
            return Err(Failure::Payload.into());
        }
        audio
            .validate_transport_wav(&bytes)
            .context(Failure::Payload)?;
        engine.check_grant_policy(g)?;
        let (cap, action, mut params) = audio.operation(role);
        params["data_base64"] = json!(STANDARD.encode(&bytes));
        let played = engine
            .execute_with_python_output_limit(
                cap,
                action,
                params,
                Some(g.limits.max_result_bytes),
                Some(execution_deadline),
            )
            .await
            .context(Failure::Playback)?;
        if played["played"] != true || played["bytes"] != manifest.payload_bytes {
            return Err(Failure::Playback.into());
        }
        engine.check_grant_policy(g)?;
        // Final ACK is deliberately withheld until the approved sink returns.
        // This is an endpoint report, not proof of physical sound or rollback.
        channel.acknowledge().await.context(Failure::Transfer)?;
        (manifest, 1 + count as u32)
    };
    Ok(json!(TransferReceipt {
        schema_version: 2,
        pair_session_id: scope.session_id.clone(),
        scope_sha256: scope.digest()?,
        role,
        payload_bytes: manifest.payload_bytes,
        payload_sha256: manifest.payload_sha256,
        data_frames: frames,
        payload_retained: false,
        sink_effects_confirmed: false,
        producer_reported: role == Role::Source,
        sink_reported: role == Role::Destination
    }))
}
