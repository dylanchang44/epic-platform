use super::{domain::*, repository::ResearchRepository, source::ResearchSource};
use crate::{state::PortfolioState, symbol::StockSymbol};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::RwLock;

pub struct ResearchService {
    repository: Result<ResearchRepository, ResearchError>,
    source: Result<ResearchSource, ResearchError>,
    refreshing: AtomicBool,
    attempts: RwLock<HashMap<StockSymbol, RefreshAttempt>>,
}

#[derive(Clone)]
struct RefreshAttempt {
    at: String,
    error: Option<ResearchError>,
}

// Admission flag, not a state lock. Drop also resets it if a request is cancelled.
struct RefreshPermit<'a>(&'a AtomicBool);
impl Drop for RefreshPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl ResearchService {
    pub async fn open(path: &Path) -> Arc<Self> {
        Self::with_source(path, ResearchSource::public_source()).await
    }

    pub async fn with_source(
        path: &Path,
        source: Result<ResearchSource, ResearchError>,
    ) -> Arc<Self> {
        Arc::new(Self {
            repository: ResearchRepository::open(path).await,
            source,
            refreshing: AtomicBool::new(false),
            attempts: RwLock::new(HashMap::new()),
        })
    }

    pub fn initialization_error(&self) -> Option<ResearchError> {
        self.repository.as_ref().err().cloned()
    }

    pub async fn company(&self, raw_symbol: &str) -> CompanyView {
        let mut view = CompanyView {
            symbol: raw_symbol.trim().to_ascii_uppercase(),
            company: None,
            status: ResearchStatus::Unsupported,
            snapshot: None,
            error: None,
            last_refresh_at: None,
            refresh_error: None,
        };
        let symbol = match StockSymbol::parse(raw_symbol) {
            Ok(symbol) => symbol,
            Err(_) => {
                view.error = Some(ResearchError::InvalidSymbol);
                return view;
            }
        };
        view.company = Company::find(&symbol);
        if view.company.is_none() {
            view.status = ResearchStatus::Unmatched;
            view.error = Some(ResearchError::UnsupportedSymbol);
            return view;
        }
        // Copy attempt metadata and drop its guard before SQLite work.
        let attempt = self.attempts.read().await.get(&symbol).cloned();
        if let Some(attempt) = attempt {
            view.last_refresh_at = Some(attempt.at);
            view.refresh_error = attempt.error;
        }
        let saved = match &self.repository {
            Ok(repository) => repository.latest(&symbol).await,
            Err(error) => Err(error.clone()),
        };
        match saved {
            Ok(snapshot) => {
                view.status = if snapshot.is_some() {
                    ResearchStatus::Available
                } else {
                    ResearchStatus::NeverRefreshed
                };
                view.snapshot = snapshot;
            }
            Err(error) => {
                view.status = ResearchStatus::RepositoryFailure;
                view.error = Some(error);
            }
        }
        view
    }

    pub async fn holdings(&self, portfolio: &PortfolioState) -> HoldingsResearch {
        // Owned projection: the portfolio read guard has already been dropped.
        let (portfolio_status, input) = portfolio.research_input().await;
        let mut result = HoldingsResearch {
            portfolio_status,
            portfolio_available: input.is_some(),
            repository_error: self.initialization_error(),
            holdings: Vec::new(),
        };
        for holding in input.unwrap_or_default() {
            let research = if let Some(symbol) = &holding.stock_symbol {
                self.company(symbol.as_str()).await
            } else {
                CompanyView {
                    symbol: holding.held_symbol.clone(),
                    company: None,
                    status: ResearchStatus::Unsupported,
                    snapshot: None,
                    error: None,
                    last_refresh_at: None,
                    refresh_error: None,
                }
            };
            if research.status == ResearchStatus::RepositoryFailure {
                result.repository_error = research.error.clone();
            }
            result.holdings.push(HoldingResearch { holding, research });
        }
        result
    }

    pub async fn refresh(&self, raw_symbol: &str) -> RefreshResponse {
        let symbol = match StockSymbol::parse(raw_symbol) {
            Ok(symbol) => symbol,
            Err(_) => {
                return RefreshResponse {
                    outcome: RefreshOutcome::Failed,
                    company: self.company(raw_symbol).await,
                };
            }
        };
        let Some(company) = Company::find(&symbol) else {
            return RefreshResponse {
                outcome: RefreshOutcome::Failed,
                company: self.company(raw_symbol).await,
            };
        };
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let mut view = self.company(raw_symbol).await;
            view.refresh_error = Some(ResearchError::Busy);
            return RefreshResponse {
                outcome: RefreshOutcome::Failed,
                company: view,
            };
        }
        let _permit = RefreshPermit(&self.refreshing);
        // No portfolio access, shared-state lock, or database transaction during HTTP.
        let result = async {
            let repository = self.repository.as_ref().map_err(Clone::clone)?;
            let source = self.source.as_ref().map_err(Clone::clone)?;
            let snapshot = source.fetch(&company).await?;
            repository.save(&snapshot).await
        }
        .await;
        self.attempts.write().await.insert(
            symbol,
            RefreshAttempt {
                at: chrono::Utc::now().to_rfc3339(),
                error: result.as_ref().err().cloned(),
            },
        );
        let outcome = match result {
            Ok(true) => RefreshOutcome::Saved,
            Ok(false) => RefreshOutcome::Unchanged,
            Err(_) => RefreshOutcome::Failed,
        };
        RefreshResponse {
            outcome,
            company: self.company(raw_symbol).await,
        }
    }
}
