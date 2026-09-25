//! Pure calculations over owned inputs. No state, database or network access.
use super::domain::*;
use rust_decimal::Decimal;
use std::collections::BTreeSet;

fn sum(mut values: impl Iterator<Item = Decimal>) -> Result<Decimal, ReviewError> {
    values.try_fold(Decimal::ZERO, |sum, value| {
        sum.checked_add(value).ok_or(ReviewError::Calculation)
    })
}

fn percentage(numerator: Decimal, denominator: Decimal) -> Result<Option<Decimal>, ReviewError> {
    if denominator <= Decimal::ZERO {
        return Ok(None);
    }
    numerator
        .checked_div(denominator)
        .and_then(|v| v.checked_mul(Decimal::ONE_HUNDRED))
        .map(|v| Some(v.round_dp(6)))
        .ok_or(ReviewError::Calculation)
}

pub fn summarize(positions: &mut [ReviewPosition]) -> Result<ReviewSummary, ReviewError> {
    if positions.is_empty() {
        return Err(ReviewError::InvalidPortfolio);
    }
    let total_market_value = sum(positions.iter().map(|p| p.market_value))?;
    let gross_market_value = sum(positions.iter().map(|p| p.market_value.abs()))?;
    for position in positions.iter_mut() {
        position.weight_percent = percentage(position.market_value, total_market_value)?;
    }
    let mut ranked: Vec<_> = positions.iter().collect();
    ranked.sort_by(|a, b| {
        b.market_value
            .abs()
            .cmp(&a.market_value.abs())
            .then(a.symbol.cmp(&b.symbol))
    });
    let first = ranked[0];
    let largest_position = LargestPosition {
        symbol: first.symbol.clone(),
        market_value: first.market_value,
        gross_weight_percent: percentage(first.market_value.abs(), gross_market_value)?,
    };
    let top_three_concentration_percent = percentage(
        sum(ranked.iter().take(3).map(|p| p.market_value.abs()))?,
        gross_market_value,
    )?;
    let supported: Vec<_> = positions
        .iter()
        .filter(|p| {
            matches!(
                p.coverage,
                CoverageStatus::Available
                    | CoverageStatus::Missing
                    | CoverageStatus::RepositoryFailure
            )
        })
        .collect();
    let covered: Vec<_> = supported
        .iter()
        .filter(|p| p.coverage == CoverageStatus::Available)
        .collect();
    let supported_gross_market_value = sum(supported.iter().map(|p| p.market_value.abs()))?;
    let covered_gross_market_value = sum(covered.iter().map(|p| p.market_value.abs()))?;
    Ok(ReviewSummary {
        total_market_value,
        gross_market_value,
        position_count: positions.len(),
        largest_position,
        top_three_concentration_percent,
        supported_position_count: supported.len(),
        covered_position_count: covered.len(),
        missing_position_count: supported.len() - covered.len(),
        unmatched_position_count: positions
            .iter()
            .filter(|p| p.coverage == CoverageStatus::Unmatched)
            .count(),
        unsupported_position_count: positions
            .iter()
            .filter(|p| p.coverage == CoverageStatus::Unsupported)
            .count(),
        supported_gross_market_value,
        covered_gross_market_value,
        coverage_count_percent: percentage(
            Decimal::from(covered.len() as u64),
            Decimal::from(supported.len() as u64),
        )?,
        coverage_value_percent: percentage(
            covered_gross_market_value,
            supported_gross_market_value,
        )?,
    })
}

pub fn compare(
    current: &SavedReview,
    previous: &SavedReview,
) -> Result<ReviewComparison, ReviewError> {
    let current_summary = &current.document.summary;
    let previous_summary = &previous.document.summary;
    let difference =
        |a: Option<Decimal>, b: Option<Decimal>| -> Result<Option<Decimal>, ReviewError> {
            match (a, b) {
                (Some(a), Some(b)) => a.checked_sub(b).map(Some).ok_or(ReviewError::Calculation),
                _ => Ok(None),
            }
        };
    let symbols = |review: &SavedReview| -> BTreeSet<String> {
        review
            .document
            .positions
            .iter()
            .map(|p| {
                p.stock_symbol
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| p.symbol.trim().to_string())
            })
            .collect()
    };
    let current_symbols = symbols(current);
    let previous_symbols = symbols(previous);
    Ok(ReviewComparison {
        previous_id: previous.id,
        total_value_change: current_summary
            .total_market_value
            .checked_sub(previous_summary.total_market_value)
            .ok_or(ReviewError::Calculation)?,
        position_count_change: i64::try_from(current_summary.position_count)
            .map_err(|_| ReviewError::Calculation)?
            .checked_sub(
                i64::try_from(previous_summary.position_count)
                    .map_err(|_| ReviewError::Calculation)?,
            )
            .ok_or(ReviewError::Calculation)?,
        previous_largest_symbol: previous_summary.largest_position.symbol.clone(),
        current_largest_symbol: current_summary.largest_position.symbol.clone(),
        concentration_change_points: difference(
            current_summary.top_three_concentration_percent,
            previous_summary.top_three_concentration_percent,
        )?,
        coverage_count_change_points: difference(
            current_summary.coverage_count_percent,
            previous_summary.coverage_count_percent,
        )?,
        coverage_value_change_points: difference(
            current_summary.coverage_value_percent,
            previous_summary.coverage_value_percent,
        )?,
        added_symbols: current_symbols
            .difference(&previous_symbols)
            .cloned()
            .collect(),
        removed_symbols: previous_symbols
            .difference(&current_symbols)
            .cloned()
            .collect(),
    })
}

#[cfg(feature = "ssr")]
pub fn age_seconds(at: &str, earlier: &str) -> Result<i64, ReviewError> {
    let at = chrono::DateTime::parse_from_rfc3339(at).map_err(|_| ReviewError::Calculation)?;
    let earlier =
        chrono::DateTime::parse_from_rfc3339(earlier).map_err(|_| ReviewError::Calculation)?;
    // Negative age honestly exposes a future provider timestamp / clock discrepancy.
    Ok(at.signed_duration_since(earlier).num_seconds())
}
