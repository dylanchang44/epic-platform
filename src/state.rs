//! One server-owned snapshot; reloads run off the async executor and publish atomically.
use crate::{
    config::AppConfig,
    portfolio::{LoadError, LoadStatus, PortfolioSnapshot, loader::load_portfolio},
};
use leptos::prelude::LeptosOptions;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub leptos_options: LeptosOptions,
    pub portfolio: Arc<PortfolioState>,
    pub research: Arc<crate::research::service::ResearchService>,
    pub review: Arc<crate::review::service::ReviewService>,
    pub jobs: Arc<crate::jobs::service::JobService>,
}

impl axum::extract::FromRef<AppState> for LeptosOptions {
    fn from_ref(state: &AppState) -> Self {
        state.leptos_options.clone()
    }
}

pub struct PortfolioState {
    config: AppConfig,
    snapshot: RwLock<PortfolioSnapshot>,
    reload_lock: Mutex<()>,
}

impl PortfolioState {
    pub fn new(config: AppConfig) -> Arc<Self> {
        Arc::new(Self {
            snapshot: RwLock::new(PortfolioSnapshot {
                data_directory: config.schwab_data_dir.to_string_lossy().into_owned(),
                status: LoadStatus::NotLoaded,
                last_successful_load: None,
                error: None,
                portfolio: None,
            }),
            config,
            reload_lock: Mutex::new(()),
        })
    }

    pub async fn snapshot(&self) -> PortfolioSnapshot {
        self.snapshot.read().await.clone()
    }

    pub async fn research_input(
        &self,
    ) -> (LoadStatus, Option<Vec<crate::portfolio::HoldingSummary>>) {
        let snapshot = self.snapshot.read().await;
        let holdings = snapshot.portfolio.as_ref().map(|portfolio| {
            portfolio
                .positions
                .iter()
                .map(|p| crate::portfolio::HoldingSummary {
                    held_symbol: p.symbol.clone(),
                    stock_symbol: p.stock_symbol(),
                    description: p.description.clone(),
                    market_value: p.market_value,
                })
                .collect()
        });
        (snapshot.status.clone(), holdings)
    }

    pub async fn reload(self: &Arc<Self>) -> PortfolioSnapshot {
        let state = self.clone();
        // An HTTP disconnect must not abandon a reload in the Loading state.
        tokio::spawn(async move {
            let _reload = state.reload_lock.lock().await;
            state.snapshot.write().await.status = LoadStatus::Loading;
            let directory = state.config.schwab_data_dir.clone();
            let result = tokio::task::spawn_blocking(move || load_portfolio(&directory))
                .await
                .unwrap_or_else(|e| {
                    Err(LoadError::new(
                        "load_failed",
                        format!("Loader task failed: {e}"),
                    ))
                });
            let mut snapshot = state.snapshot.write().await;
            match result {
                Ok(portfolio) => {
                    snapshot.portfolio = Some(portfolio);
                    snapshot.last_successful_load = Some(chrono::Utc::now().to_rfc3339());
                    snapshot.status = LoadStatus::Loaded;
                    snapshot.error = None;
                }
                Err(error) => {
                    snapshot.status = LoadStatus::Failed;
                    snapshot.error = Some(error);
                }
            }
            snapshot.clone()
        })
        .await
        .expect("portfolio reload coordinator panicked")
    }
}
