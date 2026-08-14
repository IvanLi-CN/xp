//! Bounded repository-history query selection and completeness metadata.

#![allow(dead_code)]

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

const MAX_QUERY_PAGE_SIZE: usize = 1_000;
const MAX_QUERY_RANGE_SECONDS: u64 = 2 * 365 * 24 * 60 * 60;
const MAX_QUERY_CANDIDATES: usize = 64;
const MAX_QUERY_WATERMARKS: usize = 256;
const MAX_QUERY_GAPS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueryError {
    InvalidRange,
    RangeTooLarge,
    PageSizeOutOfBounds,
    InvalidIdentifier,
    CandidateLimitExceeded,
    MetadataLimitExceeded,
    InvalidPageCursor,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "history repository query error: {self:?}")
    }
}

impl std::error::Error for QueryError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct QueryRange {
    start_unix_seconds: u64,
    end_unix_seconds: u64,
}

impl QueryRange {
    pub(crate) fn new(start_unix_seconds: u64, end_unix_seconds: u64) -> Result<Self, QueryError> {
        if start_unix_seconds > end_unix_seconds {
            return Err(QueryError::InvalidRange);
        }
        if end_unix_seconds.saturating_sub(start_unix_seconds) > MAX_QUERY_RANGE_SECONDS {
            return Err(QueryError::RangeTooLarge);
        }
        Ok(Self {
            start_unix_seconds,
            end_unix_seconds,
        })
    }

    fn covers(self, requested: Self) -> bool {
        self.start_unix_seconds <= requested.start_unix_seconds
            && self.end_unix_seconds >= requested.end_unix_seconds
    }

    pub(crate) fn start_unix_seconds(self) -> u64 {
        self.start_unix_seconds
    }

    pub(crate) fn end_unix_seconds(self) -> u64 {
        self.end_unix_seconds
    }

    fn shared_seconds(self, other: Self, requested: Self) -> u64 {
        let start = self
            .start_unix_seconds
            .max(other.start_unix_seconds)
            .max(requested.start_unix_seconds);
        let end = self
            .end_unix_seconds
            .min(other.end_unix_seconds)
            .min(requested.end_unix_seconds);
        end.checked_sub(start)
            .and_then(|span| span.checked_add(1))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryQuery {
    range: QueryRange,
    page_size: usize,
    page_cursor: usize,
}

impl HistoryQuery {
    pub(crate) fn new(
        start_unix_seconds: u64,
        end_unix_seconds: u64,
        page_size: usize,
    ) -> Result<Self, QueryError> {
        if page_size == 0 || page_size > MAX_QUERY_PAGE_SIZE {
            return Err(QueryError::PageSizeOutOfBounds);
        }
        Ok(Self {
            range: QueryRange::new(start_unix_seconds, end_unix_seconds)?,
            page_size,
            page_cursor: 0,
        })
    }

    pub(crate) fn with_page_cursor(mut self, cursor: Option<&str>) -> Result<Self, QueryError> {
        let Some(cursor) = cursor else {
            return Ok(self);
        };
        if cursor.is_empty() || !cursor.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(QueryError::InvalidPageCursor);
        }
        self.page_cursor = cursor.parse().map_err(|_| QueryError::InvalidPageCursor)?;
        Ok(self)
    }

    pub(crate) fn range(&self) -> QueryRange {
        self.range
    }

    pub(crate) fn page_size(&self) -> usize {
        self.page_size
    }

    pub(crate) fn page_offset(&self) -> Result<usize, QueryError> {
        Ok(self.page_cursor)
    }

    pub(crate) fn next_page_cursor(&self, returned_records: usize) -> Result<String, QueryError> {
        if returned_records == 0 {
            return Err(QueryError::InvalidPageCursor);
        }
        self.page_cursor
            .checked_add(returned_records)
            .ok_or(QueryError::InvalidPageCursor)
            .map(|cursor| cursor.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct QueryCoverage {
    observed: QueryRange,
    received: QueryRange,
}

impl QueryCoverage {
    pub(crate) fn new(observed: QueryRange, received: QueryRange) -> Self {
        Self { observed, received }
    }

    pub(crate) fn observed(&self) -> QueryRange {
        self.observed
    }

    pub(crate) fn received(&self) -> QueryRange {
        self.received
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StreamWatermark {
    source_node_id: String,
    source_epoch: u64,
    stream: String,
    sequence: u64,
}

impl StreamWatermark {
    pub(crate) fn new(
        source_node_id: impl Into<String>,
        source_epoch: u64,
        stream: impl Into<String>,
        sequence: u64,
    ) -> Result<Self, QueryError> {
        let source_node_id = source_node_id.into();
        let stream = stream.into();
        validate_identifier(&source_node_id)?;
        validate_identifier(&stream)?;
        Ok(Self {
            source_node_id,
            source_epoch,
            stream,
            sequence,
        })
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn source_node_id(&self) -> &str {
        &self.source_node_id
    }

    pub(crate) fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    pub(crate) fn stream(&self) -> &str {
        &self.stream
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct QueryGap {
    range: QueryRange,
    permanent: bool,
}

impl QueryGap {
    pub(crate) fn new(
        start_unix_seconds: u64,
        end_unix_seconds: u64,
        permanent: bool,
    ) -> Result<Self, QueryError> {
        Ok(Self {
            range: QueryRange::new(start_unix_seconds, end_unix_seconds)?,
            permanent,
        })
    }

    pub(crate) fn permanent(&self) -> bool {
        self.permanent
    }

    fn intersects(&self, requested: QueryRange) -> bool {
        self.range.start_unix_seconds <= requested.end_unix_seconds
            && requested.start_unix_seconds <= self.range.end_unix_seconds
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CandidateState {
    Ready,
    Local,
    Unavailable,
    Unready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryCandidate {
    repository_id: String,
    state: CandidateState,
    coverage: Option<QueryCoverage>,
    watermarks: Vec<StreamWatermark>,
    gaps: Vec<QueryGap>,
    clock_skew_seconds: i64,
}

impl QueryCandidate {
    pub(crate) fn ready(
        repository_id: impl Into<String>,
        coverage: QueryCoverage,
        watermarks: impl IntoIterator<Item = StreamWatermark>,
        gaps: impl IntoIterator<Item = QueryGap>,
        clock_skew_seconds: i64,
    ) -> Result<Self, QueryError> {
        let repository_id = repository_id.into();
        validate_identifier(&repository_id)?;
        let mut watermarks = bounded_metadata(watermarks, MAX_QUERY_WATERMARKS)?;
        let mut gaps = bounded_metadata(gaps, MAX_QUERY_GAPS)?;
        watermarks.sort_by(|left, right| watermark_key(left).cmp(&watermark_key(right)));
        watermarks.dedup_by(|left, right| watermark_key(left) == watermark_key(right));
        gaps.sort_by_key(|gap| gap.range);
        gaps.dedup();
        Ok(Self {
            repository_id,
            state: CandidateState::Ready,
            coverage: Some(coverage),
            watermarks,
            gaps,
            clock_skew_seconds,
        })
    }

    pub(crate) fn unavailable(repository_id: impl Into<String>) -> Self {
        Self::not_ready(repository_id, CandidateState::Unavailable)
    }

    pub(crate) fn local(
        coverage: QueryCoverage,
        watermarks: impl IntoIterator<Item = StreamWatermark>,
        gaps: impl IntoIterator<Item = QueryGap>,
        clock_skew_seconds: i64,
    ) -> Result<Self, QueryError> {
        let mut watermarks = bounded_metadata(watermarks, MAX_QUERY_WATERMARKS)?;
        let mut gaps = bounded_metadata(gaps, MAX_QUERY_GAPS)?;
        watermarks.sort_by(|left, right| watermark_key(left).cmp(&watermark_key(right)));
        watermarks.dedup_by(|left, right| watermark_key(left) == watermark_key(right));
        gaps.sort_by_key(|gap| gap.range);
        gaps.dedup();
        Ok(Self {
            repository_id: String::new(),
            state: CandidateState::Local,
            coverage: Some(coverage),
            watermarks,
            gaps,
            clock_skew_seconds,
        })
    }

    pub(crate) fn unready(repository_id: impl Into<String>) -> Self {
        Self::not_ready(repository_id, CandidateState::Unready)
    }

    fn not_ready(repository_id: impl Into<String>, state: CandidateState) -> Self {
        Self {
            repository_id: repository_id.into(),
            state,
            coverage: None,
            watermarks: Vec::new(),
            gaps: Vec::new(),
            clock_skew_seconds: 0,
        }
    }

    fn coverage_seconds(&self, range: QueryRange) -> u64 {
        self.coverage.as_ref().map_or(0, |coverage| {
            coverage.observed.shared_seconds(coverage.received, range)
        })
    }

    fn covers(&self, range: QueryRange) -> bool {
        self.coverage_seconds(range) == range.shared_seconds(range, range)
    }

    fn relevant_gap_count(&self, range: QueryRange) -> usize {
        self.gaps.iter().filter(|gap| gap.intersects(range)).count()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Completeness {
    Complete,
    Partial,
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct QueryPlan {
    #[serde(rename = "repository")]
    repository_id: Option<String>,
    completeness: Completeness,
    coverage: Option<QueryCoverage>,
    watermarks: Vec<StreamWatermark>,
    gaps: Vec<QueryGap>,
    clock_skew_seconds: i64,
    page_size: usize,
}

impl QueryPlan {
    pub(crate) fn repository_id(&self) -> Option<&str> {
        self.repository_id.as_deref()
    }

    pub(crate) fn completeness(&self) -> Completeness {
        self.completeness
    }

    pub(crate) fn coverage(&self) -> Option<&QueryCoverage> {
        self.coverage.as_ref()
    }

    pub(crate) fn watermarks(&self) -> &[StreamWatermark] {
        &self.watermarks
    }

    pub(crate) fn gaps(&self) -> &[QueryGap] {
        &self.gaps
    }

    pub(crate) fn clock_skew_seconds(&self) -> i64 {
        self.clock_skew_seconds
    }

    pub(crate) fn page_size(&self) -> usize {
        self.page_size
    }
}

pub(crate) struct QuerySelector;

impl QuerySelector {
    pub(crate) fn select(
        request: &HistoryQuery,
        candidates: impl IntoIterator<Item = QueryCandidate>,
    ) -> Result<QueryPlan, QueryError> {
        let mut selected = None;
        let mut local = None;
        for (index, candidate) in candidates.into_iter().enumerate() {
            if index == MAX_QUERY_CANDIDATES {
                return Err(QueryError::CandidateLimitExceeded);
            }
            if candidate.state == CandidateState::Local {
                local = Some(candidate);
                continue;
            }
            if candidate.state == CandidateState::Ready
                && selected.as_ref().is_none_or(|current| {
                    candidate_order(request.range(), &candidate, current) == Ordering::Less
                })
            {
                selected = Some(candidate);
            }
        }
        if let Some(candidate) = selected {
            let complete = candidate.relevant_gap_count(request.range()) == 0
                && candidate.covers(request.range());
            return Ok(QueryPlan {
                repository_id: Some(candidate.repository_id),
                completeness: if complete {
                    Completeness::Complete
                } else {
                    Completeness::Partial
                },
                coverage: candidate.coverage,
                watermarks: candidate.watermarks,
                gaps: candidate.gaps,
                clock_skew_seconds: candidate.clock_skew_seconds,
                page_size: request.page_size,
            });
        }
        if let Some(candidate) = local {
            return Ok(QueryPlan {
                repository_id: None,
                completeness: Completeness::LocalOnly,
                coverage: candidate.coverage,
                watermarks: candidate.watermarks,
                gaps: candidate.gaps,
                clock_skew_seconds: candidate.clock_skew_seconds,
                page_size: request.page_size,
            });
        }
        Ok(QueryPlan {
            repository_id: None,
            completeness: Completeness::LocalOnly,
            coverage: None,
            watermarks: Vec::new(),
            gaps: Vec::new(),
            clock_skew_seconds: 0,
            page_size: request.page_size,
        })
    }
}

fn candidate_order(request: QueryRange, left: &QueryCandidate, right: &QueryCandidate) -> Ordering {
    let left_complete = left.relevant_gap_count(request) == 0 && left.covers(request);
    let right_complete = right.relevant_gap_count(request) == 0 && right.covers(request);
    right_complete
        .cmp(&left_complete)
        .then_with(|| {
            right
                .coverage_seconds(request)
                .cmp(&left.coverage_seconds(request))
        })
        .then_with(|| {
            left.relevant_gap_count(request)
                .cmp(&right.relevant_gap_count(request))
        })
        .then_with(|| {
            left.clock_skew_seconds
                .unsigned_abs()
                .cmp(&right.clock_skew_seconds.unsigned_abs())
        })
        .then_with(|| left.repository_id.cmp(&right.repository_id))
}

fn bounded_metadata<T>(
    items: impl IntoIterator<Item = T>,
    limit: usize,
) -> Result<Vec<T>, QueryError> {
    let mut bounded = Vec::with_capacity(limit);
    for item in items {
        if bounded.len() == limit {
            return Err(QueryError::MetadataLimitExceeded);
        }
        bounded.push(item);
    }
    Ok(bounded)
}

fn watermark_key(watermark: &StreamWatermark) -> (&str, u64, &str) {
    (
        watermark.source_node_id.as_str(),
        watermark.source_epoch,
        watermark.stream.as_str(),
    )
}

fn validate_identifier(value: &str) -> Result<(), QueryError> {
    if value.trim().is_empty() || value != value.trim() || value.chars().any(char::is_control) {
        return Err(QueryError::InvalidIdentifier);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
