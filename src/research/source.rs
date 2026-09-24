//! Minimal port of ConsensX market.rs at 99c8e68: public forecast page,
//! bounded SvelteKit/devalue decoding, and reported-quarter facts only.
use super::domain::{Company, EarningsPeriod, ResearchError, ResearchSnapshot, SourceLink};
use reqwest::{Client, Url};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{str::FromStr, time::Duration};

const SOURCE_BASE: &str = "https://stockanalysis.com/";
pub const SOURCE_TIMEOUT: Duration = Duration::from_secs(25);

pub struct ResearchSource {
    http: Client,
    base: Url,
}

impl ResearchSource {
    pub fn public_source() -> Result<Self, ResearchError> {
        Self::with_base_url(SOURCE_BASE, SOURCE_TIMEOUT)
    }

    /// Dependency injection for offline HTTP tests; no configurable second provider.
    pub fn with_base_url(base: &str, timeout: Duration) -> Result<Self, ResearchError> {
        let base = Url::parse(base).map_err(|_| ResearchError::SourceUnavailable)?;
        let http = Client::builder()
            .timeout(timeout)
            .user_agent("EPIC-Platform/0.1 (personal earnings dashboard; ConsensX adapter)")
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| ResearchError::SourceUnavailable)?;
        Ok(Self { http, base })
    }

    pub async fn fetch(&self, company: &Company) -> Result<ResearchSnapshot, ResearchError> {
        let path = format!(
            "stocks/{}/forecast/__data.json",
            company.symbol.as_str().to_ascii_lowercase()
        );
        let url = self
            .base
            .join(&path)
            .map_err(|_| ResearchError::SourceUnavailable)?;
        let mut response = self.http.get(url).send().await.map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(ResearchError::SourceHttp {
                status: response.status().as_u16(),
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len() + chunk.len() > 2_000_000 {
                return Err(ResearchError::MalformedSource);
            }
            bytes.extend_from_slice(&chunk);
        }
        normalize_response(company, &bytes, &chrono::Utc::now().to_rfc3339())
    }
}

fn transport_error(error: reqwest::Error) -> ResearchError {
    if error.is_timeout() {
        ResearchError::Timeout
    } else {
        ResearchError::SourceUnavailable
    }
}

/// This is the external-response -> internal-domain boundary. Values from the
/// provider's reference tables never become API/view models directly.
pub fn normalize_response(
    company: &Company,
    bytes: &[u8],
    retrieved_at: &str,
) -> Result<ResearchSnapshot, ResearchError> {
    let payload: Value =
        serde_json::from_slice(bytes).map_err(|_| ResearchError::MalformedSource)?;
    let forecast = decode_page(&payload)?;
    let table = &forecast["estimates"]["table"]["quarterly"];
    let index = table["lastDate"]
        .as_u64()
        .filter(|i| *i < 100)
        .ok_or(ResearchError::MalformedSource)? as usize;
    let year = text(&table["fiscalYear"][index])?;
    let quarter = text(&table["fiscalQuarter"][index])?;
    if year.len() != 4
        || !year.bytes().all(|b| b.is_ascii_digit())
        || !matches!(quarter, "Q1" | "Q2" | "Q3" | "Q4")
        || forecast["priceTargets"]["currency"] != "USD"
    {
        return Err(ResearchError::MalformedSource);
    }
    let source_updated_at = forecast["trust"]["lastUpdated"]
        .as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .ok_or(ResearchError::MalformedSource)?
        .to_rfc3339();
    let snapshot = ResearchSnapshot {
        company: company.clone(),
        period: EarningsPeriod {
            label: format!("FY{year} {quarter}"),
            ended_on: text(&table["dates"][index])?.into(),
        },
        revenue: decimal(&table["revenue"][index])?,
        diluted_eps: decimal(&table["eps"][index])?,
        currency: "USD".into(),
        source_updated_at,
        retrieved_at: retrieved_at.into(),
        sources: vec![SourceLink {
            label: "Stock Analysis · reported quarterly results (S&P Global)".into(),
            url: format!(
                "{SOURCE_BASE}stocks/{}/forecast/",
                company.symbol.as_str().to_ascii_lowercase()
            ),
        }],
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn text(value: &Value) -> Result<&str, ResearchError> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && *s != "[PRO]")
        .ok_or(ResearchError::MalformedSource)
}

fn decimal(value: &Value) -> Result<Decimal, ResearchError> {
    let number = value
        .as_number()
        .ok_or(ResearchError::MalformedSource)?
        .to_string();
    Decimal::from_str(&number)
        .or_else(|_| Decimal::from_scientific(&number))
        .map_err(|_| ResearchError::MalformedSource)
}

fn decode_page(payload: &Value) -> Result<Value, ResearchError> {
    let table = payload["nodes"]
        .as_array()
        .and_then(|nodes| nodes.last())
        .and_then(|node| node["data"].as_array())
        .ok_or(ResearchError::MalformedSource)?;
    expand(table, 0, 0, &mut 50_000)
}

// Resolve references as data, never evaluate source JavaScript. ConsensX's
// recursion and node budgets prevent cyclic or excessively expanded input.
fn expand(
    table: &[Value],
    index: i64,
    depth: usize,
    budget: &mut usize,
) -> Result<Value, ResearchError> {
    if depth >= 40 || *budget == 0 {
        return Err(ResearchError::MalformedSource);
    }
    *budget -= 1;
    if index == -1 {
        return Ok(Value::Null);
    }
    let value = usize::try_from(index)
        .ok()
        .and_then(|i| table.get(i))
        .ok_or(ResearchError::MalformedSource)?;
    Ok(match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    Ok((
                        key.clone(),
                        expand(
                            table,
                            value.as_i64().ok_or(ResearchError::MalformedSource)?,
                            depth + 1,
                            budget,
                        )?,
                    ))
                })
                .collect::<Result<_, ResearchError>>()?,
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| {
                    expand(
                        table,
                        value.as_i64().ok_or(ResearchError::MalformedSource)?,
                        depth + 1,
                        budget,
                    )
                })
                .collect::<Result<_, _>>()?,
        ),
        other => other.clone(),
    })
}
