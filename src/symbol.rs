//! Small shared identity; instrument classification remains in portfolio.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StockSymbol(String);

impl StockSymbol {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim().to_ascii_uppercase();
        let parts: Vec<_> = value.split(['.', '-']).collect();
        if value.is_empty()
            || value.len() > 16
            || parts.len() > 2
            || !parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err("Expected an ordinary stock symbol (letters, optionally a dot or dash share class).".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for StockSymbol {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<StockSymbol> for String {
    fn from(value: StockSymbol) -> Self {
        value.0
    }
}

impl std::fmt::Display for StockSymbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
