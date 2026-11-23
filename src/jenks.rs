use std::cmp::Ordering;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasher, Hasher};

const MAX_SPLIT_ENTRIES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
struct SeededU64BuildHasher {
    seed: u64,
}

impl Default for SeededU64BuildHasher {
    fn default() -> Self {
        let state = RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_u64(0x9e37_79b9_7f4a_7c15);
        Self {
            seed: hasher.finish(),
        }
    }
}

struct SeededU64Hasher {
    state: u64,
}

impl Hasher for SeededU64Hasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state = self.state.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ u64::from(byte);
        }
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        // Per-instance seeded SplitMix64 mixes IEEE-754 exponent/mantissa bits evenly
        // into both hashbrown bucket indices and control tags without external crates.
        let mut mixed = value ^ self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        self.state = mixed ^ (mixed >> 31);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.state
    }
}

impl BuildHasher for SeededU64BuildHasher {
    type Hasher = SeededU64Hasher;

    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        SeededU64Hasher { state: self.seed }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JenksError {
    InvalidBreaks,
    NonFiniteValue(f64),
    CountOverflow,
    Internal(&'static str),
}

impl fmt::Display for JenksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBreaks => write!(f, "breaks must be greater than or equal to 1"),
            Self::NonFiniteValue(value) => write!(f, "Jenks input must be finite f64, got {value}"),
            Self::CountOverflow => write!(f, "too many Jenks input values"),
            Self::Internal(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for JenksError {}

pub type JenksResult<T> = Result<T, JenksError>;

#[derive(Clone, Debug, Default)]
pub struct JenksCounts {
    counts: HashMap<u64, u64, SeededU64BuildHasher>,
    total_count: u64,
}

impl JenksCounts {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            counts: HashMap::with_capacity_and_hasher(capacity, SeededU64BuildHasher::default()),
            total_count: 0,
        }
    }

    pub fn push(&mut self, value: f64) -> JenksResult<()> {
        let key = finite_f64_key(value)?;
        let count = self.counts.entry(key).or_insert(0);
        *count = count.checked_add(1).ok_or(JenksError::CountOverflow)?;
        self.total_count = self
            .total_count
            .checked_add(1)
            .ok_or(JenksError::CountOverflow)?;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.total_count == 0
    }

    pub fn distinct_len(&self) -> usize {
        self.counts.len()
    }

    fn sorted_values(&self) -> Vec<(f64, u64)> {
        let mut values: Vec<_> = self
            .counts
            .iter()
            .map(|(key, count)| (f64::from_bits(*key), *count))
            .collect();
        values.sort_by(|(left, _), (right, _)| left.partial_cmp(right).unwrap_or(Ordering::Equal));
        values
    }

    fn all_counts_are_one(&self) -> bool {
        self.total_count == self.counts.len() as u64
    }
}

pub fn breaks_from_values<I>(values: I, breaks: i32, invert: bool) -> JenksResult<Option<Vec<f64>>>
where
    I: IntoIterator<Item = f64>,
{
    if breaks < 1 {
        return Err(JenksError::InvalidBreaks);
    }
    let iter = values.into_iter();
    let (lower, _) = iter.size_hint();
    let mut counts = JenksCounts::with_capacity(lower.min(4096));
    for value in iter {
        counts.push(value)?;
    }
    breaks_from_counts(&counts, breaks, invert)
}

pub fn breaks_from_counts(
    counts: &JenksCounts,
    breaks: i32,
    invert: bool,
) -> JenksResult<Option<Vec<f64>>> {
    if breaks < 1 {
        return Err(JenksError::InvalidBreaks);
    }
    if counts.is_empty() {
        return Ok(None);
    }

    let values = counts.sorted_values();
    let distinct = values.len();
    let class_count = breaks as usize;
    if distinct <= class_count {
        return Ok(Some(values.into_iter().map(|(value, _)| value).collect()));
    }
    if class_count == 1 {
        let edge = if invert {
            values[0].0
        } else {
            values[distinct - 1].0
        };
        return Ok(Some(vec![edge]));
    }

    let split_entries = (class_count - 1).checked_mul(distinct + 1);
    let use_low_memory_fallback = counts.all_counts_are_one()
        && (distinct > u32::MAX as usize
            || split_entries.is_none_or(|entries| entries > MAX_SPLIT_ENTRIES));
    if use_low_memory_fallback {
        let normalized = normalize_sorted_values(&values);
        if let Ok(ranges) = natural_breaks::classify_indices(&normalized, class_count) {
            return Ok(Some(edges_from_value_ranges(&values, &ranges, invert)));
        }
    }

    weighted_jenks_edges(&values, class_count, invert).map(Some)
}

fn finite_f64_key(value: f64) -> JenksResult<u64> {
    if !value.is_finite() {
        return Err(JenksError::NonFiniteValue(value));
    }
    let normalized = if value == 0.0 { 0.0 } else { value };
    Ok(normalized.to_bits())
}

fn edges_from_value_ranges(
    values: &[(f64, u64)],
    ranges: &[(usize, usize)],
    invert: bool,
) -> Vec<f64> {
    ranges
        .iter()
        .map(|(start, end)| {
            if invert {
                values[*start].0
            } else {
                values[end - 1].0
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
enum ValueScaler {
    ShiftScale { min: f64, span: f64 },
    HalfShiftScale { half_min: f64, half_span: f64 },
}

impl ValueScaler {
    fn new(min: f64, max: f64) -> Self {
        let span = max - min;
        if span.is_finite() {
            Self::ShiftScale { min, span }
        } else {
            let half_min = min * 0.5;
            let half_max = max * 0.5;
            Self::HalfShiftScale {
                half_min,
                half_span: half_max - half_min,
            }
        }
    }

    #[inline(always)]
    fn normalize(self, value: f64) -> f64 {
        match self {
            Self::ShiftScale { min, span } => (value - min) / span,
            Self::HalfShiftScale {
                half_min,
                half_span,
            } => (value * 0.5 - half_min) / half_span,
        }
    }
}

fn normalize_sorted_values(values: &[(f64, u64)]) -> Vec<f64> {
    let scaler = ValueScaler::new(values[0].0, values[values.len() - 1].0);
    values
        .iter()
        .map(|(value, _)| scaler.normalize(*value))
        .collect()
}

#[derive(Clone, Copy, Default)]
struct Prefix {
    weight: f64,
    sum: f64,
    sum_sq: f64,
}

fn weighted_jenks_edges(
    values: &[(f64, u64)],
    class_count: usize,
    invert: bool,
) -> JenksResult<Vec<f64>> {
    let n = values.len();
    if class_count == 0 || class_count > n {
        return Err(JenksError::Internal("invalid Jenks class count"));
    }
    if n > u32::MAX as usize {
        return Err(JenksError::Internal("Jenks distinct value count is too large"));
    }

    // Shift and scale into [0, 1] (or [-1, 1] when max - min overflows) so variance
    // prefix sums remain exact without cancellation, overflow, or underflow.
    let scaler = ValueScaler::new(values[0].0, values[n - 1].0);
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(Prefix::default());
    let mut acc = Prefix::default();

    for (value, count) in values {
        let normalized = scaler.normalize(*value);
        let weight = *count as f64;
        let weighted = normalized * weight;
        acc.weight += weight;
        acc.sum += weighted;
        acc.sum_sq += normalized * weighted;
        prefix.push(acc);
    }

    let mut dp_prev = vec![f64::INFINITY; n + 1];
    let mut dp_cur = vec![f64::INFINITY; n + 1];
    for (i, cost) in dp_prev.iter_mut().enumerate().skip(1) {
        *cost = range_variance(prefix[0], prefix[i]);
    }

    let stride = n + 1;
    let split_count = (class_count - 1)
        .checked_mul(stride)
        .ok_or(JenksError::Internal("Jenks split matrix is too large"))?;
    let mut splits = vec![0u32; split_count];
    for class_idx in 2..=class_count {
        dp_cur.fill(f64::INFINITY);
        let row_offset = (class_idx - 2) * stride;
        compute_dp_row(
            class_idx,
            class_idx,
            n,
            class_idx - 1,
            n - 1,
            &dp_prev,
            &mut dp_cur,
            &mut splits[row_offset..row_offset + stride],
            &prefix,
        );
        std::mem::swap(&mut dp_prev, &mut dp_cur);
    }

    let mut ranges = Vec::with_capacity(class_count);
    let mut end = n;
    for class_idx in (2..=class_count).rev() {
        let split = splits[(class_idx - 2) * stride + end] as usize;
        if split >= end {
            return Err(JenksError::Internal("failed to reconstruct Jenks classes"));
        }
        ranges.push((split, end));
        end = split;
    }
    ranges.push((0, end));
    ranges.reverse();

    Ok(ranges
        .iter()
        .map(|(start, end)| values[if invert { *start } else { end - 1 }].0)
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn compute_dp_row(
    class_idx: usize,
    left: usize,
    right: usize,
    opt_left: usize,
    opt_right: usize,
    dp_prev: &[f64],
    dp_cur: &mut [f64],
    split_row: &mut [u32],
    prefix: &[Prefix],
) {
    if left > right {
        return;
    }

    let mid = left + (right - left) / 2;
    let max_split = opt_right.min(mid - 1);
    let min_split = opt_left.max(class_idx - 1);
    let mut best_cost = f64::INFINITY;
    let mut best_split = min_split;
    let right_prefix = prefix[mid];

    for split in min_split..=max_split {
        let cost = dp_prev[split] + range_variance(prefix[split], right_prefix);
        if cost < best_cost {
            best_cost = cost;
            best_split = split;
        }
    }

    dp_cur[mid] = best_cost;
    split_row[mid] = best_split as u32;

    if left < mid {
        compute_dp_row(
            class_idx,
            left,
            mid - 1,
            opt_left,
            best_split,
            dp_prev,
            dp_cur,
            split_row,
            prefix,
        );
    }
    if mid < right {
        compute_dp_row(
            class_idx,
            mid + 1,
            right,
            best_split,
            opt_right,
            dp_prev,
            dp_cur,
            split_row,
            prefix,
        );
    }
}

#[inline(always)]
fn range_variance(left: Prefix, right: Prefix) -> f64 {
    let weight = right.weight - left.weight;
    if weight <= 0.0 {
        return 0.0;
    }
    let sum = right.sum - left.sum;
    let sum_sq = right.sum_sq - left.sum_sq;
    (sum_sq - (sum * sum / weight)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unwrap_breaks(values: &[f64], breaks: i32, invert: bool) -> Vec<f64> {
        breaks_from_values(values.iter().copied(), breaks, invert)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn exact_edges_match_known_clusters() {
        let values = [1.0, 2.0, 3.0, 10.0, 11.0, 12.0];
        assert_eq!(unwrap_breaks(&values, 2, false), vec![3.0, 12.0]);
    }

    #[test]
    fn lower_edges_are_returned_when_inverted() {
        let values = [1.0, 2.0, 3.0, 10.0, 11.0, 12.0];
        assert_eq!(unwrap_breaks(&values, 2, true), vec![1.0, 10.0]);
    }

    #[test]
    fn duplicate_heavy_inputs_are_weighted() {
        let values = [1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 50.0, 50.0, 51.0, 51.0, 51.0];
        assert_eq!(unwrap_breaks(&values, 2, false), vec![2.0, 51.0]);
        assert_eq!(unwrap_breaks(&values, 2, true), vec![1.0, 50.0]);
        assert_eq!(unwrap_breaks(&values, 1, false), vec![51.0]);
        assert_eq!(unwrap_breaks(&values, 3, true), vec![1.0, 2.0, 50.0]);
    }

    #[test]
    fn distinct_count_shortcut_returns_sorted_unique_values() {
        let values = [3.0, 1.0, 3.0, 2.0, 1.0];
        assert_eq!(unwrap_breaks(&values, 4, false), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(
            breaks_from_values(std::iter::empty::<f64>(), 3, false).unwrap(),
            None
        );
    }

    #[test]
    fn invalid_breaks_error() {
        assert!(matches!(
            breaks_from_values([1.0, 2.0], 0, false),
            Err(JenksError::InvalidBreaks)
        ));
    }

    #[test]
    fn non_finite_values_error() {
        assert!(matches!(
            breaks_from_values([1.0, f64::NAN], 2, false),
            Err(JenksError::NonFiniteValue(_))
        ));
        assert!(matches!(
            breaks_from_values([1.0, f64::INFINITY], 2, false),
            Err(JenksError::NonFiniteValue(_))
        ));
    }

    #[test]
    fn natural_breaks_path_matches_weighted_fallback_on_unique_values() {
        let values = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];
        let ranges = natural_breaks::classify_indices(&values, 3).unwrap();
        let pairs: Vec<(f64, u64)> = values.iter().map(|&v| (v, 1)).collect();
        assert_eq!(
            unwrap_breaks(&values, 3, false),
            edges_from_value_ranges(&pairs, &ranges, false)
        );
    }

    #[test]
    fn shifted_and_extreme_ranges_preserve_exact_clusters() {
        let shifted_weighted = [
            1_000_000_001.0,
            1_000_000_001.0,
            1_000_000_002.0,
            1_000_000_003.0,
            1_000_000_010.0,
            1_000_000_011.0,
            1_000_000_012.0,
        ];
        assert_eq!(
            unwrap_breaks(&shifted_weighted, 2, false),
            vec![1_000_000_003.0, 1_000_000_012.0]
        );
        assert_eq!(
            unwrap_breaks(&shifted_weighted[1..], 2, false),
            vec![1_000_000_003.0, 1_000_000_012.0]
        );

        let huge = [1e200, 1e200, 2e200, 3e200, 1e201, 1.1e201, 1.2e201];
        assert_eq!(unwrap_breaks(&huge, 2, false), vec![3e200, 1.2e201]);

        let tiny = [1e-200, 1e-200, 2e-200, 3e-200, 1e-199, 1.1e-199, 1.2e-199];
        assert_eq!(unwrap_breaks(&tiny, 2, false), vec![3e-200, 1.2e-199]);

        let full_span = [-1e308, -0.99e308, -0.98e308, 0.98e308, 0.99e308, 1e308];
        assert_eq!(unwrap_breaks(&full_span, 2, false), vec![-0.98e308, 1e308]);

        let asymmetric_span = [
            -0.1e308, -0.09e308, -0.08e308, 1.73e308, 1.74e308, 1.75e308,
        ];
        assert_eq!(
            unwrap_breaks(&asymmetric_span, 2, false),
            vec![-0.08e308, 1.75e308]
        );
    }
}
