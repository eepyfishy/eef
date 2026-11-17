//! Independently negotiated, historical analysis reads over the existing pinned
//! control channel. Reading never changes job tracking or execution authority.
use crate::{
    grants::{
        GrantLedger,
        stop::Scope,
        workload::{VISION_MAX_PAGE, VisionPage},
    },
    workload_leases::definition::Definition,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub available: bool,
    pub max_page_size: usize,
    pub read_only: bool,
}
impl Advertisement {
    pub fn new(available: bool) -> Self {
        Self {
            schema_version: 1,
            available,
            max_page_size: VISION_MAX_PAGE,
            read_only: true,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.max_page_size == VISION_MAX_PAGE && self.read_only,
            "Unsupported retained vision result inspection; no fallback"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub id: String,
    pub scope: Scope,
    pub after_sequence: u64,
    pub limit: usize,
    pub expected_prefix_sha256: String,
}
impl WireRequest {
    pub fn validate(&self, issuer: &str, node: &str) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        self.scope.validate()?;
        ensure!(
            self.kind == "workload_analyses"
                && self.schema_version == 1
                && self.scope.issuer_id == issuer
                && self.scope.executor_node == node
                && self.after_sequence <= 100
                && (1..=VISION_MAX_PAGE).contains(&self.limit)
                && crate::request_result::valid_digest(&self.expected_prefix_sha256)
                && (self.after_sequence != 0
                    || self.expected_prefix_sha256
                        == hex::encode(Sha256::digest(self.scope.grant_sha256.as_bytes()))),
            "Vision inspection requires its original issuer, exact node and bounded prefix"
        );
        Ok(())
    }
    pub fn read(&self, ledger: &GrantLedger, issuer: &str, node: &str) -> Result<VisionPage> {
        self.validate(issuer, node)?;
        let receipt = ledger.workload_receipt(issuer, &self.scope.grant_id)?;
        ensure!(
            receipt.definition.scope()? == self.scope,
            "Retained vision scope differs from issued workload"
        );
        ledger.workload_analysis_page(
            issuer,
            &receipt.definition,
            self.after_sequence,
            self.limit,
            &self.expected_prefix_sha256,
        )
    }
}
impl crate::NodeServer {
    pub async fn inspect_workload_analyses(
        &self,
        d: &Definition,
        after: u64,
        limit: usize,
        previous: &str,
    ) -> Result<VisionPage> {
        VisionPage::seed(d)?;
        let scope = d.scope()?;
        let mut request = serde_json::to_value(WireRequest {
            kind: "workload_analyses".into(),
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            scope,
            after_sequence: after,
            limit,
            expected_prefix_sha256: previous.into(),
        })?;
        let parsed: WireRequest = serde_json::from_value(request.clone())?;
        parsed.validate(&d.issuer_id, &d.executor_node)?;
        request["expected_node_id"] = serde_json::json!(d.executor_node);
        request["expected_issuer_id"] = serde_json::json!(d.issuer_id);
        let response = self
            .invoke_message(
                &d.executor_node,
                request,
                std::time::Duration::from_secs(10),
                None,
                None,
                None,
            )
            .await?;
        ensure!(
            response["success"] == true,
            "Retained vision result unavailable; no replay or fallback"
        );
        let page: VisionPage = serde_json::from_value(response["data"].clone())?;
        page.validate(d, after, limit, previous)?;
        Ok(page)
    }
}
