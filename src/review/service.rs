use super::{calculations, domain::*, repository::ReviewRepository};
use crate::{
    research::{domain::Company, service::ResearchService},
    state::PortfolioState,
};
use std::{collections::HashMap, path::Path, sync::Arc};

pub struct ReviewService {
    repository: Result<ReviewRepository, ReviewError>,
}

impl ReviewService {
    pub async fn open(path: &Path) -> Arc<Self> {
        Arc::new(Self {
            repository: ReviewRepository::open(path).await,
        })
    }

    pub fn initialization_error(&self) -> Option<ReviewError> {
        self.repository.as_ref().err().cloned()
    }

    pub async fn create_portfolio_review(
        &self,
        portfolio: &PortfolioState,
        research: &ResearchService,
    ) -> Result<SavedReview, ReviewError> {
        // snapshot() clones under its own short read guard; no guard leaves Portfolio.
        let input = portfolio.snapshot().await;
        let portfolio_captured_at = chrono::Utc::now().to_rfc3339();
        let portfolio = input.portfolio.ok_or(ReviewError::NoPortfolio)?;
        let loaded_at = input
            .last_successful_load
            .ok_or(ReviewError::InvalidPortfolio)?;
        if portfolio.positions.is_empty()
            || portfolio.positions.len() != portfolio.summary.position_count
        {
            return Err(ReviewError::InvalidPortfolio);
        }
        let repository = self.repository.as_ref().map_err(Clone::clone)?;
        let symbols: Vec<_> = portfolio
            .positions
            .iter()
            .filter_map(|p| p.stock_symbol())
            .filter(|s| Company::find(s).is_some())
            .collect();
        let saved = research.latest_saved(&symbols).await;
        let research_captured_at = chrono::Utc::now().to_rfc3339();
        let research_error = saved.as_ref().err().cloned();
        let saved: HashMap<_, _> = saved
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.snapshot.company.symbol.clone(), s))
            .collect();
        let created_at = chrono::Utc::now().to_rfc3339();
        calculations::age_seconds(&created_at, &loaded_at)?;
        let mut positions = Vec::new();
        for position in portfolio.positions {
            let symbol = position.stock_symbol();
            let supported = symbol.as_ref().is_some_and(|s| Company::find(s).is_some());
            // Copy even when differently cased source rows normalize to one company.
            let snapshot = symbol.as_ref().and_then(|s| saved.get(s)).cloned();
            let coverage = if symbol.is_none() {
                CoverageStatus::Unsupported
            } else if !supported {
                CoverageStatus::Unmatched
            } else if research_error.is_some() {
                CoverageStatus::RepositoryFailure
            } else if snapshot.is_some() {
                CoverageStatus::Available
            } else {
                CoverageStatus::Missing
            };
            let research = snapshot
                .map(|saved| {
                    Ok::<_, ReviewError>(ReviewResearch {
                        snapshot_id: saved.id,
                        retrieval_age_seconds: calculations::age_seconds(
                            &created_at,
                            &saved.snapshot.retrieved_at,
                        )?,
                        source_age_seconds: calculations::age_seconds(
                            &created_at,
                            &saved.snapshot.source_updated_at,
                        )?,
                        snapshot: saved.snapshot,
                    })
                })
                .transpose()?;
            positions.push(ReviewPosition {
                symbol: position.symbol,
                stock_symbol: symbol,
                description: position.description,
                asset_type: position.asset_type,
                quantity: position.quantity,
                market_value: position.market_value,
                weight_percent: None,
                coverage,
                research,
            });
        }
        let summary = calculations::summarize(&mut positions)?;
        if summary.total_market_value != portfolio.summary.total_market_value {
            return Err(ReviewError::InvalidPortfolio);
        }
        repository
            .save(ReviewDocument {
                calculation_version: 1,
                created_at,
                portfolio_captured_at,
                research_captured_at,
                portfolio_loaded_at: loaded_at,
                portfolio_load_status: input.status,
                portfolio_source_file: portfolio.source_file,
                research_error,
                summary,
                positions,
            })
            .await
    }

    pub async fn history(&self) -> Result<Vec<ReviewHistoryEntry>, ReviewError> {
        self.repository
            .as_ref()
            .map_err(Clone::clone)?
            .history()
            .await
    }

    pub async fn detail(&self, id: i64) -> Result<ReviewDetail, ReviewError> {
        if id <= 0 {
            return Err(ReviewError::InvalidId);
        }
        let repository = self.repository.as_ref().map_err(Clone::clone)?;
        let review = repository.get(id).await?;
        // A comparison failure does not hide the selected immutable record.
        let comparison = match repository.previous(id).await {
            Ok(Some(previous)) => calculations::compare(&review, &previous).map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        Ok(ReviewDetail {
            review,
            comparison_error: comparison.as_ref().err().cloned(),
            comparison: comparison.unwrap_or_default(),
        })
    }
}
