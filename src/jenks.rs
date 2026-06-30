use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

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
    counts: HashMap<u64, u64>,
    total_count: u64,
}

impl JenksCounts {
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
    let mut counts = JenksCounts::default();
    for value in values {
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

    if counts.all_counts_are_one() {
        let sorted: Vec<f64> = values.iter().map(|(value, _)| *value).collect();
        if let Ok(ranges) = natural_breaks::classify_indices(&sorted, class_count) {
            return Ok(Some(edges_from_ranges(&sorted, &ranges, invert)));
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

fn edges_from_ranges(values: &[f64], ranges: &[(usize, usize)], invert: bool) -> Vec<f64> {
    ranges
        .iter()
        .map(|(start, end)| {
            if invert {
                values[*start]
            } else {
                values[end - 1]
            }
        })
        .collect()
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

    let mut prefix_weight = vec![0.0; n + 1];
    let mut prefix_sum = vec![0.0; n + 1];
    let mut prefix_sum_sq = vec![0.0; n + 1];

    for (idx, (value, count)) in values.iter().enumerate() {
        let i = idx + 1;
        let weight = *count as f64;
        prefix_weight[i] = prefix_weight[idx] + weight;
        prefix_sum[i] = prefix_sum[idx] + value * weight;
        prefix_sum_sq[i] = prefix_sum_sq[idx] + value * value * weight;
    }

    let mut dp_prev = vec![f64::INFINITY; n + 1];
    let mut dp_cur = vec![f64::INFINITY; n + 1];
    for (i, cost) in dp_prev.iter_mut().enumerate().skip(1) {
        *cost = range_variance(&prefix_weight, &prefix_sum, &prefix_sum_sq, 0, i);
    }

    let mut splits = vec![vec![0usize; n + 1]; class_count + 1];
    for class_idx in 2..=class_count {
        dp_cur.fill(f64::INFINITY);
        compute_dp_row(
            class_idx,
            class_idx,
            n,
            class_idx - 1,
            n - 1,
            &dp_prev,
            &mut dp_cur,
            &mut splits[class_idx],
            &prefix_weight,
            &prefix_sum,
            &prefix_sum_sq,
        );
        std::mem::swap(&mut dp_prev, &mut dp_cur);
    }

    let mut ranges = Vec::with_capacity(class_count);
    let mut end = n;
    for class_idx in (2..=class_count).rev() {
        let split = splits[class_idx][end];
        if split >= end {
            return Err(JenksError::Internal("failed to reconstruct Jenks classes"));
        }
        ranges.push((split, end));
        end = split;
    }
    ranges.push((0, end));
    ranges.reverse();

    Ok(edges_from_ranges(
        &values.iter().map(|(value, _)| *value).collect::<Vec<_>>(),
        &ranges,
        invert,
    ))
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
    split_row: &mut [usize],
    prefix_weight: &[f64],
    prefix_sum: &[f64],
    prefix_sum_sq: &[f64],
) {
    if left > right {
        return;
    }

    let mid = left + (right - left) / 2;
    let max_split = opt_right.min(mid - 1);
    let min_split = opt_left.max(class_idx - 1);
    let mut best_cost = f64::INFINITY;
    let mut best_split = min_split;

    for split in min_split..=max_split {
        let cost =
            dp_prev[split] + range_variance(prefix_weight, prefix_sum, prefix_sum_sq, split, mid);
        if cost < best_cost {
            best_cost = cost;
            best_split = split;
        }
    }

    dp_cur[mid] = best_cost;
    split_row[mid] = best_split;

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
            prefix_weight,
            prefix_sum,
            prefix_sum_sq,
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
            prefix_weight,
            prefix_sum,
            prefix_sum_sq,
        );
    }
}

fn range_variance(
    prefix_weight: &[f64],
    prefix_sum: &[f64],
    prefix_sum_sq: &[f64],
    start: usize,
    end: usize,
) -> f64 {
    let weight = prefix_weight[end] - prefix_weight[start];
    if weight <= 0.0 {
        return 0.0;
    }
    let sum = prefix_sum[end] - prefix_sum[start];
    let sum_sq = prefix_sum_sq[end] - prefix_sum_sq[start];
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
        assert_eq!(
            unwrap_breaks(&values, 3, false),
            edges_from_ranges(&values, &ranges, false)
        );
    }
}
