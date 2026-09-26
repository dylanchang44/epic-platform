use super::{domain::*, repository::BriefingRepository};
use crate::research::{domain::Company, service::ResearchService};

pub struct WatchlistService {
    configuration: Result<WatchlistInput, WatchlistError>,
    repository: Result<BriefingRepository, WatchlistError>,
}
#[derive(Clone, Copy)]
pub enum BriefingStep {
    ReadingWatchlist,
    LoadingResearch,
    PersistingBriefing,
}
impl WatchlistService {
    pub fn new(
        pool: Result<sqlx::SqlitePool, WatchlistError>,
        configuration: Result<WatchlistInput, WatchlistError>,
    ) -> Self {
        Self {
            repository: pool.map(BriefingRepository::new),
            configuration,
        }
    }
    pub fn configuration(&self) -> WatchlistConfiguration {
        WatchlistConfiguration {
            input: self.configuration.as_ref().ok().cloned(),
            error: self.configuration.as_ref().err().cloned(),
        }
    }
    pub fn submission_input(&self) -> Result<WatchlistInput, WatchlistError> {
        let input = self.configuration.clone()?;
        input.validate_submission()?;
        Ok(input)
    }
    pub fn repository(&self) -> Result<&BriefingRepository, WatchlistError> {
        self.repository.as_ref().map_err(Clone::clone)
    }
    pub async fn create_for_job<F, Fut>(
        &self,
        research: &ResearchService,
        input: WatchlistInput,
        job_id: i64,
        mut progress: F,
    ) -> Result<SavedBriefing, WatchlistError>
    where
        F: FnMut(BriefingStep) -> Fut,
        Fut: std::future::Future<Output = Result<(), WatchlistError>>,
    {
        let repository = self.repository()?;
        if let Some(saved) = repository.by_origin_job(job_id).await? {
            return Ok(saved);
        }
        progress(BriefingStep::ReadingWatchlist).await?;
        input.validate_submission()?;
        progress(BriefingStep::LoadingResearch).await?;
        let snapshots = research
            .latest_saved(input.symbols())
            .await
            .map_err(|_| WatchlistError::ResearchUnavailable)?;
        let entries = input
            .symbols()
            .iter()
            .map(|symbol| {
                let research = snapshots
                    .iter()
                    .find(|s| &s.snapshot.company.symbol == symbol)
                    .cloned();
                BriefingEntry {
                    symbol: symbol.clone(),
                    company_name: research
                        .as_ref()
                        .map(|s| s.snapshot.company.name.clone())
                        .or_else(|| Company::find(symbol).map(|c| c.name)),
                    status: if research.is_some() {
                        BriefingStatus::Available
                    } else {
                        BriefingStatus::NeverRefreshed
                    },
                    research,
                }
            })
            .collect();
        let document = BriefingDocument {
            created_at: chrono::Utc::now().to_rfc3339(),
            entries,
        };
        progress(BriefingStep::PersistingBriefing).await?;
        repository.save(document, job_id).await
    }
}
