//! Read-only owned-job observations, not RAM/VRAM estimates or remote attestation.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobAccounting {
    Available {
        #[serde(deserialize_with = "required_limit")]
        job_commit_limit_bytes: Option<u64>,
        peak_job_commit_bytes: u64,
        active_processes: u32,
        total_user_cpu_100ns: u64,
        total_kernel_cpu_100ns: u64,
    },
    NoOwnedJob {},
    Unsupported {},
    Unavailable {},
}
fn required_limit<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(deserializer)
}
impl JobAccounting {
    pub fn validate(&self) -> bool {
        match self {
            Self::Available {
                job_commit_limit_bytes,
                ..
            } => job_commit_limit_bytes.is_none_or(|bytes| {
                bytes % (1024 * 1024) == 0
                    && crate::model_memory::validate(Some(bytes / (1024 * 1024))).is_ok()
            }),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unavailable_states_cannot_claim_zero_usage_or_extra_metrics() {
        for state in ["no_owned_job", "unsupported", "unavailable"] {
            let sample: JobAccounting = serde_json::from_value(json!({"state":state})).unwrap();
            assert!(sample.validate());
            assert!(
                serde_json::from_value::<JobAccounting>(
                    json!({"state":state,"peak_job_commit_bytes":0})
                )
                .is_err()
            );
        }
        assert!(serde_json::from_value::<JobAccounting>(json!({"state":"estimated"})).is_err());
    }
    #[test]
    fn observed_limits_are_bounded_commit_bytes_not_memory_estimates() {
        for (limit, valid) in [
            (None, true),
            (Some(64 * 1024 * 1024), true),
            (Some(0), false),
            (Some(64 * 1024 * 1024 + 1), false),
        ] {
            let sample = JobAccounting::Available {
                job_commit_limit_bytes: limit,
                peak_job_commit_bytes: 8 * 1024 * 1024,
                active_processes: 2,
                total_user_cpu_100ns: 1,
                total_kernel_cpu_100ns: 2,
            };
            assert_eq!(sample.validate(), valid);
        }
        assert!(serde_json::from_value::<JobAccounting>(json!({"state":"available","job_commit_limit_bytes":null,
            "peak_job_commit_bytes":-1,"active_processes":1,"total_user_cpu_100ns":0,"total_kernel_cpu_100ns":0})).is_err());
        assert!(serde_json::from_value::<JobAccounting>(json!({"state":"available",
            "peak_job_commit_bytes":1,"active_processes":1,"total_user_cpu_100ns":0,"total_kernel_cpu_100ns":0})).is_err());
    }
}
