use super::{LoadError, Portfolio, PortfolioSummary, Position};
use rust_decimal::Decimal;
use std::collections::HashSet;

// Numeric absence is explicit. Malformed values must never silently become zero.
fn number(value: &str) -> Result<Option<Decimal>, String> {
    let value = value.trim();
    if ["", "--", "N/A", "null"].contains(&value) {
        return Ok(None);
    }
    let (negative, value) = if value.starts_with('(') && value.ends_with(')') {
        (true, &value[1..value.len() - 1])
    } else if let Some(value) = value.strip_prefix('-') {
        (true, value)
    } else {
        (false, value)
    };
    let value = value.strip_prefix('$').unwrap_or(value);
    let (negative, value) = if let Some(value) = value.strip_prefix('-') {
        if negative {
            return Err("invalid sign".into());
        }
        (true, value)
    } else {
        (negative, value)
    };
    let integer = value.split('.').next().unwrap_or_default();
    if integer.contains(',') {
        let groups: Vec<_> = integer.split(',').collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return Err("invalid thousands grouping".into());
        }
    }
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
        || value.split('.').count() > 2
        || value.split('.').nth(1).is_some_and(|s| s.contains(','))
    {
        return Err("invalid decimal".into());
    }
    let cleaned = value.replace(',', "");
    let decimal = Decimal::from_str_exact(&cleaned)
        .map_err(|_| "invalid or out-of-range decimal".to_string())?;
    Ok(Some(if negative { -decimal } else { decimal }))
}

pub fn parse_positions(content: &str, source_file: &str) -> Result<Portfolio, LoadError> {
    let fail =
        |message: String| LoadError::new("positions_invalid", format!("{source_file}: {message}"));
    if content.trim().is_empty() {
        return Err(fail("positions file is empty".into()));
    }
    let content = content.trim_start_matches('\u{feff}');
    validate_quotes(content).map_err(fail)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(content.as_bytes());
    let mut headers: Option<csv::StringRecord> = None;
    let mut positions = Vec::new();
    let mut symbols = HashSet::new();
    let mut total = Decimal::ZERO;
    let mut reported_total = None;
    for row in reader.records() {
        let row = row.map_err(|e| fail(format!("malformed CSV: {e}")))?;
        let line = row.position().map(|p| p.line()).unwrap_or(0);
        let first = row.get(0).unwrap_or_default().trim();
        if headers.is_none() {
            if first == "Symbol" {
                let normalized: csv::StringRecord = row
                    .iter()
                    .map(|s| s.trim().split(" (").next().unwrap_or(s))
                    .collect();
                for required in ["Symbol", "Description", "Qty", "Price", "Mkt Val"] {
                    if normalized.iter().filter(|h| *h == required).count() != 1 {
                        return Err(fail(format!(
                            "missing or duplicate required column: {required}"
                        )));
                    }
                }
                headers = Some(normalized);
            }
            continue;
        }
        let header = headers.as_ref().unwrap();
        if row.iter().all(|s| s.trim().is_empty()) {
            continue;
        }
        let cell = |name: &str| {
            header
                .iter()
                .position(|h| h == name)
                .and_then(|i| row.get(i))
                .unwrap_or("")
                .trim()
        };
        let numeric = |name: &str| {
            number(cell(name)).map_err(|reason| fail(format!("row {line}, {name}: {reason}")))
        };
        if first.contains("Positions Total") || first.contains("Account Total") {
            if let Some(value) = numeric("Mkt Val")? {
                reported_total = Some(value);
            }
            continue;
        }
        // Schwab sometimes adds one empty trailing field. No other width mismatch is accepted.
        let effective_len = |record: &csv::StringRecord| {
            record
                .iter()
                .rev()
                .skip_while(|s| s.trim().is_empty())
                .count()
        };
        if effective_len(&row) > header.len() || row.len() < effective_len(header) {
            return Err(fail(format!(
                "row {line}: column count does not match header"
            )));
        }
        if first.is_empty() || first == "Symbol" || !symbols.insert(first.to_string()) {
            return Err(fail(format!("row {line}: missing or duplicate symbol")));
        }
        let cash = first.eq_ignore_ascii_case("Cash & Cash Investments")
            || first.eq_ignore_ascii_case("Cash");
        let quantity = numeric("Qty")?;
        let price = numeric("Price")?;
        if !cash && (quantity.is_none() || price.is_none()) {
            return Err(fail(format!(
                "row {line}: quantity and price are required for non-cash positions"
            )));
        }
        let market_value = numeric("Mkt Val")?
            .ok_or_else(|| fail(format!("row {line}: market value is required")))?;
        total = total
            .checked_add(market_value)
            .ok_or_else(|| fail("market-value sum overflowed".into()))?;
        positions.push(Position {
            symbol: first.into(),
            description: cell("Description").into(),
            quantity,
            price,
            market_value,
            cost_basis: numeric("Cost Basis")?,
            asset_type: cell("Asset Type").into(),
        });
    }
    if headers.is_none() {
        return Err(fail("missing Symbol header".into()));
    }
    if positions.is_empty() {
        return Err(fail("no holdings found".into()));
    }
    if reported_total.is_some_and(|reported| reported != total) {
        return Err(fail(
            "exported total does not match the sum of holding market values".into(),
        ));
    }
    Ok(Portfolio {
        summary: PortfolioSummary {
            position_count: positions.len(),
            total_market_value: total,
        },
        positions,
        source_file: source_file.into(),
    })
}

// The csv crate intentionally tolerates malformed quoting. Reject it before
// parsing so a truncated quoted field cannot become an apparently valid holding.
fn validate_quotes(content: &str) -> Result<(), String> {
    enum Field {
        Start,
        Bare,
        Quoted,
        Closed,
    }
    let mut state = Field::Start;
    for byte in content.bytes() {
        state = match (state, byte) {
            (Field::Start, b'"') => Field::Quoted,
            (Field::Quoted, b'"') => Field::Closed,
            (Field::Quoted, _) => Field::Quoted,
            (Field::Closed, b'"') => Field::Quoted,
            (Field::Closed | Field::Bare | Field::Start, b',' | b'\n' | b'\r') => Field::Start,
            (Field::Start | Field::Bare, byte) if byte != b'"' => Field::Bare,
            _ => return Err("malformed CSV quoting".into()),
        };
    }
    if matches!(state, Field::Quoted) {
        return Err("unterminated quoted CSV field".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_signed_currency_and_missing_values_without_rounding() {
        assert_eq!(
            number("($1,234.56)").unwrap(),
            Some(Decimal::new(-123456, 2))
        );
        assert_eq!(number("-$0.10").unwrap(), Some(Decimal::new(-10, 2)));
        assert_eq!(number("--").unwrap(), None);
        for invalid in ["NaN", "1garbage", "1,2", "1.2,3", "--1", "1.2.3"] {
            assert!(number(invalid).is_err());
        }
    }
}
