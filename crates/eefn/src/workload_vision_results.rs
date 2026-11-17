//! Bounded historical values. These can be deserialized, but cannot construct
//! an analysis permit, owned source witness, lease or execution authority.
use super::*;

pub const MAX_PAGE: usize = 16;
pub const MAX_PAGE_BYTES: usize = 128 * 1024 - 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub(super) intent: Intent,
    pub(super) intent_sha256: String,
    pub(super) result: Option<AnalysisResult>,
    pub(super) result_sha256: Option<String>,
}
impl Record {
    pub fn sequence(&self) -> u64 {
        self.intent.sequence
    }
    pub fn text(&self) -> Option<&str> {
        self.result.as_ref().map(|r| r.text.as_str())
    }
    pub(super) fn next_prefix(&self, previous: &str) -> String {
        hash(
            format!(
                "{}{}{}",
                previous,
                self.intent_sha256,
                self.result_sha256.as_deref().unwrap_or("pending")
            )
            .as_bytes(),
        )
    }
    fn validate(
        &self,
        d: &Definition,
        receipt: &crate::request_receipt::ConsumedReceipt,
    ) -> Result<()> {
        let p = parameters(d)?;
        let i = &self.intent;
        ensure!(
            i.schema_version == 1
                && i.grant_sha256 == d.scope()?.grant_sha256
                && (1..=p.max_calls()).contains(&i.sequence)
                && (2..=u64::from(p.capture.max_frames)).contains(&i.source_sequence)
                && (4..=p.images.max_image_bytes).contains(&i.image_bytes)
                && crate::request_result::valid_digest(&i.image_sha256)
                && i.lease_sequence > 0
                && i.recorded_at_ms >= receipt.accepted_at_ms
                && i.recorded_at_ms <= receipt.updated_at_ms
                && serde_json::to_vec(i)?.len() <= HEADROOM
                && self.intent_sha256 == hash(&serde_json::to_vec(i)?),
            "Invalid retained analysis intent"
        );
        match (&self.result, &self.result_sha256) {
            (None, None) => {}
            (Some(result), Some(checksum)) => ensure!(
                result.schema_version == 1
                    && result.sequence == i.sequence
                    && result.intent_sha256 == self.intent_sha256
                    && result.lease_sequence >= i.lease_sequence
                    && result.recorded_at_ms >= i.recorded_at_ms
                    && result.recorded_at_ms <= receipt.updated_at_ms
                    && !result.text.trim().is_empty()
                    && !result.text.contains('\0')
                    && serde_json::to_vec(&result.text)?.len() <= p.inference.max_result_bytes
                    && serde_json::to_vec(result)?.len() <= p.inference.max_result_bytes + HEADROOM
                    && *checksum == hash(&serde_json::to_vec(result)?),
                "Invalid retained analysis result"
            ),
            _ => bail!("Incomplete retained analysis checksum"),
        }
        Ok(())
    }
}
impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedVisionRecord")
            .field("sequence", &self.intent.sequence)
            .field("result_retained", &self.result.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub schema_version: u32,
    pub grant_id: String,
    pub grant_sha256: String,
    pub receipt: crate::request_receipt::ConsumedReceipt,
    pub completion: Option<super::super::CompletionReport>,
    pub summary: Summary,
    pub after_sequence: u64,
    pub prefix_before_sha256: String,
    pub analyses: Vec<Record>,
    pub next_sequence: u64,
    pub prefix_after_sha256: String,
    pub more: bool,
    pub execution_authorized: bool,
    pub restart_authorized: bool,
    pub model_replay_authorized: bool,
}
impl Page {
    pub fn seed(d: &Definition) -> Result<String> {
        parameters(d)?;
        Ok(hash(d.scope()?.grant_sha256.as_bytes()))
    }
    pub fn validate(&self, d: &Definition, after: u64, limit: usize, previous: &str) -> Result<()> {
        let p = parameters(d)?;
        let scope = d.scope()?;
        self.receipt.validate()?;
        let s = &self.summary;
        ensure!(
            (1..=MAX_PAGE).contains(&limit)
                && after <= p.max_calls()
                && crate::request_result::valid_digest(previous)
                && (after != 0 || previous == Self::seed(d)?)
                && self.schema_version == 2
                && self.grant_id == d.grant_id
                && self.grant_sha256 == scope.grant_sha256
                && self.receipt.node_id == d.executor_node
                && self.receipt.issuer_id == d.issuer_id
                && self.receipt.job_id == d.job_id
                && self.receipt.task_id == d.task_id
                && self.receipt.attempt == d.attempt
                && self.receipt.grant_id == d.grant_id
                && self.receipt.grant_sha256 == scope.grant_sha256
                && self.receipt.pipeline_progress.is_none()
                && self.after_sequence == after
                && after <= s.calls
                && self.prefix_before_sha256 == previous
                && s.calls <= p.max_calls()
                && s.completed <= s.calls
                && s.calls - s.completed <= 1
                && s.reserved_output_tokens
                    == s.calls
                        .checked_mul(p.inference.max_tokens)
                        .context("Vision token reservation overflow")?
                && s.result_bytes
                    <= usize::try_from(s.completed)?
                        .checked_mul(p.inference.max_result_bytes)
                        .context("Vision result size overflow")?
                && s.result_bytes >= usize::try_from(s.completed)?.saturating_mul(3)
                && crate::request_result::valid_digest(&s.prefix_sha256)
                && !self.execution_authorized
                && !self.restart_authorized
                && !self.model_replay_authorized
                && self.analyses.len() <= limit
                && self.next_sequence == s.calls.min(after.saturating_add(limit as u64))
                && self.analyses.len() as u64 == self.next_sequence - after
                && self.more == (self.next_sequence < s.calls)
                && serde_json::to_vec(self)?.len() <= MAX_PAGE_BYTES,
            "Invalid retained analysis page scope or budget"
        );
        super::super::Receipt {
            schema_version: if self.completion.is_some() { 3 } else { 2 },
            receipt: self.receipt.clone(),
            definition: d.clone(),
            execution_authorized: false,
            restart_authorized: false,
            completion: self.completion.clone(),
        }
        .validate()?;
        if let Some(completion) = &self.completion {
            ensure!(
                completion.analysis.as_ref() == Some(s),
                "Analysis page differs from its joined completion"
            );
        }
        let mut prefix = previous.to_owned();
        let mut page_result_bytes = 0usize;
        let mut last_source = 0;
        let mut last_lease = 0;
        let mut last_time = self.receipt.accepted_at_ms;
        for (offset, row) in self.analyses.iter().enumerate() {
            row.validate(d, &self.receipt)?;
            ensure!(
                row.intent.sequence == after + offset as u64 + 1
                    && row.intent.source_sequence > last_source
                    && row.intent.lease_sequence >= last_lease
                    && row.intent.recorded_at_ms >= last_time
                    && (row.result.is_some() || row.intent.sequence == s.calls),
                "Analysis page order changed"
            );
            last_source = row.intent.source_sequence;
            last_lease = row
                .result
                .as_ref()
                .map_or(row.intent.lease_sequence, |r| r.lease_sequence);
            last_time = row
                .result
                .as_ref()
                .map_or(row.intent.recorded_at_ms, |r| r.recorded_at_ms);
            if let Some(result) = &row.result {
                page_result_bytes = page_result_bytes
                    .checked_add(serde_json::to_vec(&result.text)?.len())
                    .context("Analysis page size overflow")?;
            }
            prefix = row.next_prefix(&prefix);
        }
        ensure!(
            self.prefix_after_sha256 == prefix && (self.more || prefix == s.prefix_sha256),
            "Analysis page prefix changed"
        );
        if s.calls == 0 {
            ensure!(
                s.last_source_sequence == 0
                    && s.last_lease_sequence == 0
                    && s.prefix_sha256 == Self::seed(d)?,
                "Invalid empty analysis history"
            );
        } else {
            ensure!(
                (2..=u64::from(p.capture.max_frames)).contains(&s.last_source_sequence)
                    && s.last_lease_sequence > 0,
                "Invalid final analysis position"
            );
        }
        if let Some(last) = self.analyses.last().filter(|_| !self.more) {
            ensure!(
                last.intent.source_sequence == s.last_source_sequence
                    && last_lease == s.last_lease_sequence
                    && s.completed == s.calls - u64::from(last.result.is_none()),
                "Analysis page last position differs from summary"
            );
        }
        ensure!(
            page_result_bytes <= s.result_bytes
                && (after != 0 || self.more || page_result_bytes == s.result_bytes),
            "Analysis page result bytes differ from retained summary"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub schema_version: u32,
    pub receipt: crate::request_receipt::ConsumedReceipt,
    pub completion: super::super::CompletionReport,
    pub summary: Summary,
    pub analyses: Vec<Record>,
}
/// A bounded historical read, including pending/unknown outcomes. Unlike
/// History, this value does not assert node completion or coordinator recovery.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    pub schema_version: u32,
    pub receipt: crate::request_receipt::ConsumedReceipt,
    pub completion: Option<super::super::CompletionReport>,
    pub summary: Summary,
    pub analyses: Vec<Record>,
}
impl Inspection {
    pub fn validate(&self, d: &Definition) -> Result<()> {
        ensure!(
            self.schema_version == 1
                && self.analyses.len() as u64 == self.summary.calls
                && self.summary.calls <= parameters(d)?.max_calls()
                && serde_json::to_vec(self)?.len() <= History::max_bytes(d)?,
            "Invalid retained analysis inspection size or count"
        );
        let mut prefix = Page::seed(d)?;
        let mut after = 0;
        let mut previous_source = 0;
        let mut previous_lease = 0;
        let mut previous_time = self.receipt.accepted_at_ms;
        let mut bytes = 0usize;
        let mut completed = 0u64;
        loop {
            let end = self.analyses.len().min(after + MAX_PAGE);
            let records = &self.analyses[after..end];
            let before = prefix.clone();
            for row in records {
                ensure!(
                    row.intent.source_sequence > previous_source
                        && row.intent.lease_sequence >= previous_lease
                        && row.intent.recorded_at_ms >= previous_time,
                    "Analysis inspection order changed across pages"
                );
                previous_source = row.intent.source_sequence;
                previous_lease = row
                    .result
                    .as_ref()
                    .map_or(row.intent.lease_sequence, |r| r.lease_sequence);
                previous_time = row
                    .result
                    .as_ref()
                    .map_or(row.intent.recorded_at_ms, |r| r.recorded_at_ms);
                if let Some(result) = &row.result {
                    completed += 1;
                    bytes = bytes
                        .checked_add(serde_json::to_vec(&result.text)?.len())
                        .context("Inspection result size overflow")?;
                }
                prefix = row.next_prefix(&prefix);
            }
            // Reuse the strict wire validator, without widening its page bound
            // or constructing a completion for pending/unknown node evidence.
            Page {
                schema_version: 2,
                grant_id: d.grant_id.clone(),
                grant_sha256: d.scope()?.grant_sha256,
                receipt: self.receipt.clone(),
                completion: self.completion.clone(),
                summary: self.summary.clone(),
                after_sequence: after as u64,
                prefix_before_sha256: before.clone(),
                analyses: records.to_vec(),
                next_sequence: end as u64,
                prefix_after_sha256: prefix.clone(),
                more: end < self.analyses.len(),
                execution_authorized: false,
                restart_authorized: false,
                model_replay_authorized: false,
            }
            .validate(d, after as u64, MAX_PAGE, &before)?;
            after = end;
            if after == self.analyses.len() {
                break;
            }
        }
        ensure!(
            completed == self.summary.completed
                && bytes == self.summary.result_bytes
                && previous_source == self.summary.last_source_sequence
                && previous_lease == self.summary.last_lease_sequence
                && prefix == self.summary.prefix_sha256,
            "Analysis inspection differs from its complete retained prefix"
        );
        Ok(())
    }
    pub fn from_pages(d: &Definition, pages: Vec<Page>) -> Result<Self> {
        ensure!(
            !pages.is_empty()
                && pages.len()
                    <= parameters(d)?.max_calls().div_ceil(MAX_PAGE as u64).max(1) as usize,
            "Invalid retained analysis inspection page count"
        );
        let first = &pages[0];
        let mut result = Self {
            schema_version: 1,
            receipt: first.receipt.clone(),
            completion: first.completion.clone(),
            summary: first.summary.clone(),
            analyses: Vec::new(),
        };
        let count = pages.len();
        let mut after = 0;
        let mut prefix = Page::seed(d)?;
        for (index, page) in pages.into_iter().enumerate() {
            page.validate(d, after, MAX_PAGE, &prefix)?;
            ensure!(
                page.receipt == result.receipt
                    && page.summary == result.summary
                    && serde_json::to_value(&page.completion)?
                        == serde_json::to_value(&result.completion)?
                    && page.more == (index + 1 < count),
                "Analysis inspection changed between pages"
            );
            after = page.next_sequence;
            prefix = page.prefix_after_sha256;
            result.analyses.extend(page.analyses);
        }
        result.validate(d)?;
        Ok(result)
    }
}
impl History {
    pub fn validate(&self, d: &Definition) -> Result<()> {
        let p = parameters(d)?;
        ensure!(
            self.schema_version == 1
                && self.receipt.state == "completed"
                && self.analyses.len() as u64 == self.summary.calls
                && self.analyses.len() as u64 <= p.max_calls(),
            "Completed analysis history is incomplete"
        );
        // Apply the same record and full-prefix checks without widening the
        // bounded wire page. Complete history is a separately bounded value.
        let mut prefix = Page::seed(d)?;
        let mut previous_source = 0;
        let mut previous_lease = 0;
        let mut previous_time = self.receipt.accepted_at_ms;
        let mut bytes = 0usize;
        for (offset, row) in self.analyses.iter().enumerate() {
            row.validate(d, &self.receipt)?;
            let result = row
                .result
                .as_ref()
                .context("Completed history has pending analysis")?;
            ensure!(
                row.intent.sequence == offset as u64 + 1
                    && row.intent.source_sequence > previous_source
                    && row.intent.lease_sequence >= previous_lease
                    && row.intent.recorded_at_ms >= previous_time,
                "Completed analysis history order changed"
            );
            previous_source = row.intent.source_sequence;
            previous_lease = result.lease_sequence;
            previous_time = result.recorded_at_ms;
            bytes = bytes
                .checked_add(serde_json::to_vec(&result.text)?.len())
                .context("Analysis history size overflow")?;
            prefix = row.next_prefix(&prefix);
        }
        super::super::Receipt {
            schema_version: 3,
            receipt: self.receipt.clone(),
            definition: d.clone(),
            execution_authorized: false,
            restart_authorized: false,
            completion: Some(self.completion.clone()),
        }
        .validate()?;
        self.summary.validate_complete(
            d,
            self.analyses.len() as u64,
            self.completion.lease_sequence,
        )?;
        ensure!(
            self.completion.analysis.as_ref() == Some(&self.summary)
                && bytes == self.summary.result_bytes
                && prefix == self.summary.prefix_sha256
                && previous_source == self.summary.last_source_sequence
                && previous_lease == self.summary.last_lease_sequence
                && serde_json::to_vec(self)?.len() <= Self::max_bytes(d)?,
            "Analysis history differs from joined completion"
        );
        Ok(())
    }
    pub fn max_bytes(d: &Definition) -> Result<usize> {
        let p = parameters(d)?;
        p.max_total_result_bytes()?
            .checked_add(
                usize::try_from(p.max_calls())?
                    .checked_mul(2 * HEADROOM)
                    .context("Analysis history reservation overflow")?,
            )
            .and_then(|n| n.checked_add(16 * 1024))
            .context("Analysis history reservation overflow")
    }
    pub fn from_pages(d: &Definition, pages: Vec<Page>) -> Result<Self> {
        ensure!(
            !pages.is_empty()
                && pages.len()
                    <= parameters(d)?.max_calls().div_ceil(MAX_PAGE as u64).max(1) as usize,
            "Invalid completed analysis page count"
        );
        let first = &pages[0];
        let mut result = Self {
            schema_version: 1,
            receipt: first.receipt.clone(),
            completion: first
                .completion
                .clone()
                .context("Analysis completion unavailable")?,
            summary: first.summary.clone(),
            analyses: Vec::new(),
        };
        let mut after = 0;
        let mut prefix = Page::seed(d)?;
        let count = pages.len();
        for (index, page) in pages.into_iter().enumerate() {
            page.validate(d, after, MAX_PAGE, &prefix)?;
            ensure!(
                page.receipt == result.receipt
                    && page.summary == result.summary
                    && serde_json::to_value(&page.completion)?
                        == serde_json::to_value(Some(&result.completion))?
                    && page.more == (index + 1 < count),
                "Analysis history changed between pages"
            );
            after = page.next_sequence;
            prefix = page.prefix_after_sha256;
            result.analyses.extend(page.analyses);
        }
        result.validate(d)?;
        Ok(result)
    }
}
