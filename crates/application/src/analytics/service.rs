//! [`AnalyticsService`] — KPI aggregation use cases.

use std::sync::Arc;

use chrono::{Duration, Utc};

use forgebike_domain::{
    entities::auth_identity::AuthIdentity,
    identifiers::RestaurantId,
    ports::{
        analytics_port::{
            AnalyticsRepository, ContentAnalyticsData, OverviewData, ReviewsAnalyticsData,
        },
        restaurant_repository::RestaurantRepository,
    },
};

use super::error::AnalyticsError;

// ---------------------------------------------------------------------------
// Allowed periods
// ---------------------------------------------------------------------------

const ALLOWED_PERIODS: &[u32] = &[30, 90, 365];

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct AnalyticsService {
    analytics: Arc<dyn AnalyticsRepository>,
    restaurants: Arc<dyn RestaurantRepository>,
}

impl AnalyticsService {
    pub fn new(
        analytics: Arc<dyn AnalyticsRepository>,
        restaurants: Arc<dyn RestaurantRepository>,
    ) -> Self {
        Self {
            analytics,
            restaurants,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    async fn verify_restaurant(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
    ) -> Result<(), AnalyticsError> {
        self.restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(AnalyticsError::RestaurantNotFound(restaurant_id))?;
        Ok(())
    }

    fn validate_period(period_days: u32) -> Result<(), AnalyticsError> {
        if ALLOWED_PERIODS.contains(&period_days) {
            Ok(())
        } else {
            Err(AnalyticsError::InvalidPeriod(period_days))
        }
    }

    fn since(period_days: u32) -> chrono::DateTime<Utc> {
        Utc::now() - Duration::days(i64::from(period_days))
    }

    // -----------------------------------------------------------------------
    // Use cases
    // -----------------------------------------------------------------------

    /// Return a KPI overview for the restaurant over the last `period_days` days.
    pub async fn overview(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        period_days: u32,
    ) -> Result<OverviewData, AnalyticsError> {
        Self::validate_period(period_days)?;
        self.verify_restaurant(identity, restaurant_id).await?;
        Ok(self
            .analytics
            .overview(identity.tenant_id, restaurant_id, Self::since(period_days))
            .await?)
    }

    /// Return detailed review analytics for the last `period_days` days.
    pub async fn reviews(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        period_days: u32,
    ) -> Result<ReviewsAnalyticsData, AnalyticsError> {
        Self::validate_period(period_days)?;
        self.verify_restaurant(identity, restaurant_id).await?;
        Ok(self
            .analytics
            .reviews_analytics(identity.tenant_id, restaurant_id, Self::since(period_days))
            .await?)
    }

    /// Return content-piece analytics for the last `period_days` days.
    pub async fn content(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        period_days: u32,
    ) -> Result<ContentAnalyticsData, AnalyticsError> {
        Self::validate_period(period_days)?;
        self.verify_restaurant(identity, restaurant_id).await?;
        Ok(self
            .analytics
            .content_analytics(identity.tenant_id, restaurant_id, Self::since(period_days))
            .await?)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::{DateTime, Utc};

    use forgebike_domain::{
        entities::{auth_identity::AuthIdentity, restaurant::Restaurant, user::UserRole},
        identifiers::{RestaurantId, TenantId, UserId},
        pagination::{ListParams, Page},
        ports::{
            analytics_port::{
                AnalyticsRepository, ContentAnalyticsData, OverviewData, ReviewsAnalyticsData,
            },
            restaurant_repository::{NewRestaurant, RestaurantRepository},
        },
        DomainError,
    };

    use super::{super::error::AnalyticsError, AnalyticsService};

    // -- Mock: RestaurantRepository ------------------------------------------

    struct MockRestaurants(Mutex<Vec<Restaurant>>);

    impl MockRestaurants {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl RestaurantRepository for MockRestaurants {
        async fn create(&self, n: NewRestaurant) -> Result<Restaurant, DomainError> {
            let r = Restaurant {
                id: RestaurantId::new(),
                tenant_id: n.tenant_id,
                name: n.name,
                description: n.description,
                cuisine_type: n.cuisine_type,
                address: n.address,
                phone: n.phone,
                website: n.website,
                google_place_id: None,
                yelp_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(r.clone());
            Ok(r)
        }
        async fn find_by_id(
            &self,
            tid: TenantId,
            id: RestaurantId,
        ) -> Result<Option<Restaurant>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id == id && r.tenant_id == tid)
                .cloned())
        }
        async fn list(
            &self,
            tid: TenantId,
            _p: ListParams,
        ) -> Result<Page<Restaurant>, DomainError> {
            Ok(Page {
                items: self
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.tenant_id == tid)
                    .cloned()
                    .collect(),
                next_cursor: None,
            })
        }
        async fn update(&self, r: &Restaurant) -> Result<Restaurant, DomainError> {
            Ok(r.clone())
        }
        async fn delete(&self, _t: TenantId, _id: RestaurantId) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    // -- Mock: AnalyticsRepository -------------------------------------------

    struct MockAnalytics;

    impl MockAnalytics {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self)
        }
    }

    #[async_trait]
    impl AnalyticsRepository for MockAnalytics {
        async fn overview(
            &self,
            _t: TenantId,
            _r: RestaurantId,
            _s: DateTime<Utc>,
        ) -> Result<OverviewData, DomainError> {
            Ok(OverviewData {
                total_reviews: 10,
                avg_rating: Some(4.2),
                avg_sentiment: Some(0.6),
                reviews_with_reply: 3,
                total_content: 5,
                published_content: 2,
            })
        }
        async fn reviews_analytics(
            &self,
            _t: TenantId,
            _r: RestaurantId,
            _s: DateTime<Utc>,
        ) -> Result<ReviewsAnalyticsData, DomainError> {
            Ok(ReviewsAnalyticsData {
                total_reviews: 10,
                avg_rating: Some(4.2),
                avg_sentiment: Some(0.6),
                reviews_with_reply: 3,
                rating_distribution: [("4".into(), 6i64), ("5".into(), 4i64)].into(),
                platform_breakdown: [("google".into(), 7i64), ("yelp".into(), 3i64)].into(),
            })
        }
        async fn content_analytics(
            &self,
            _t: TenantId,
            _r: RestaurantId,
            _s: DateTime<Utc>,
        ) -> Result<ContentAnalyticsData, DomainError> {
            Ok(ContentAnalyticsData {
                total: 5,
                by_status: [("draft".into(), 3i64), ("published".into(), 2i64)].into(),
                by_type: [("social_post".into(), 3i64), ("email".into(), 2i64)].into(),
            })
        }
    }

    // -- Helpers -------------------------------------------------------------

    fn identity() -> AuthIdentity {
        AuthIdentity {
            user_id: UserId::new(),
            tenant_id: TenantId::new(),
            role: UserRole::Owner,
        }
    }

    fn svc(
        restaurants: std::sync::Arc<MockRestaurants>,
        analytics: std::sync::Arc<MockAnalytics>,
    ) -> AnalyticsService {
        AnalyticsService::new(analytics as _, restaurants as _)
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn overview_returns_data_for_valid_period() {
        let restaurants = MockRestaurants::new();
        let id = identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: id.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let s = svc(restaurants, MockAnalytics::new());
        let data = s.overview(&id, r.id, 30).await.unwrap();

        assert_eq!(data.total_reviews, 10);
        assert_eq!(data.total_content, 5);
        assert_eq!(data.published_content, 2);
    }

    #[tokio::test]
    async fn reviews_returns_breakdown_for_valid_period() {
        let restaurants = MockRestaurants::new();
        let id = identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: id.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let s = svc(restaurants, MockAnalytics::new());
        let data = s.reviews(&id, r.id, 90).await.unwrap();

        assert_eq!(data.total_reviews, 10);
        assert!(data.platform_breakdown.contains_key("google"));
        assert!(data.rating_distribution.contains_key("5"));
    }

    #[tokio::test]
    async fn content_returns_breakdown_for_valid_period() {
        let restaurants = MockRestaurants::new();
        let id = identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: id.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let s = svc(restaurants, MockAnalytics::new());
        let data = s.content(&id, r.id, 365).await.unwrap();

        assert_eq!(data.total, 5);
        assert!(data.by_status.contains_key("draft"));
        assert!(data.by_type.contains_key("social_post"));
    }

    #[tokio::test]
    async fn invalid_period_returns_error() {
        let restaurants = MockRestaurants::new();
        let id = identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: id.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let s = svc(restaurants, MockAnalytics::new());

        let err = s.overview(&id, r.id, 7).await.unwrap_err();
        assert!(matches!(err, AnalyticsError::InvalidPeriod(7)));

        let err = s.overview(&id, r.id, 0).await.unwrap_err();
        assert!(matches!(err, AnalyticsError::InvalidPeriod(0)));
    }

    #[tokio::test]
    async fn wrong_tenant_returns_not_found() {
        let restaurants = MockRestaurants::new();
        let owner = identity();
        let other = identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: owner.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let s = svc(restaurants, MockAnalytics::new());
        let err = s.overview(&other, r.id, 30).await.unwrap_err();
        assert!(matches!(err, AnalyticsError::RestaurantNotFound(_)));
    }
}
