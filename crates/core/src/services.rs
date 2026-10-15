//! Сервисы, которые внедряют другие крейты (S1 §2.6).

/// Набор сервисов. `listing: Arc<dyn ListingService>` (S4) добавит навигация в S1-08.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Services {}

impl Services {
    /// Без внешних сервисов — для тестов и `--smoke`.
    pub fn null() -> Self {
        Services {}
    }
}
