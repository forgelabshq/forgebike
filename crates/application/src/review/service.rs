//! [`ReviewService`] — review sync and listing use cases.

use std::sync::Arc;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, review::Review, review::ReviewPlatform},
    identifiers::RestaurantId,
    pagination::{Cursor, Page},
    ports::{
        restaurant_repository::RestaurantRepository,
        review_fetch_port::ReviewFetchPort,
        review_repository::{ReviewListParams, ReviewRepository, UpsertReview},
    },
};

use super::{
    commands::{ReviewQuery, SyncSummary},
    error::ReviewError,
};

pub struct ReviewService {
    reviews: Arc<dyn ReviewRepository>,
    restaurants: Arc<dyn RestaurantRepository>,
    google: Arc<dyn ReviewFetchPort>,
    yelp: Arc<dyn ReviewFetchPort>,
    /// Reserved for when a `tripadvisor_location_id` column is added.
    #[allow(dead_code)]
    tripadvisor: Arc<dyn ReviewFetchPort>,
}

impl ReviewService {
    pub fn new(
        reviews: Arc<dyn ReviewRepository>,
        restaurants: Arc<dyn RestaurantRepository>,
        google: Arc<dyn ReviewFetchPort>,
        yelp: Arc<dyn ReviewFetchPort>,
        tripadvisor: Arc<dyn ReviewFetchPort>,
    ) -> Self {
        Self {
            reviews,
            restaurants,
            google,
            yelp,
            tripadvisor,
        }
    }

    // -----------------------------------------------------------------------
    // Sync
    // -----------------------------------------------------------------------

    /// Fetch reviews from all configured external platforms for a restaurant
    /// and upsert them into the database.
    ///
    /// Platforms whose external ID is not set on the restaurant are silently
    /// skipped.  Platforms whose API key is not configured return `Ok(vec![])`
    /// from the client, which is also counted as skipped without an error.
    pub async fn sync_reviews(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
    ) -> Result<SyncSummary, ReviewError> {
        // Verify the restaurant exists and belongs to this tenant.
        let restaurant = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(ReviewError::RestaurantNotFound(restaurant_id))?;

        let mut synced = 0u32;
        let mut platforms = vec![];
        let mut warnings = vec![];

        // -- Google ----------------------------------------------------------
        if let Some(place_id) = &restaurant.google_place_id {
            platforms.push("google".into());
            match self.google.fetch_reviews(place_id).await {
                Ok(fetched) => {
                    for r in fetched {
                        self.reviews
                            .upsert(UpsertReview {
                                restaurant_id,
                                tenant_id: identity.tenant_id,
                                platform: ReviewPlatform::Google,
                                external_id: r.external_id,
                                author_name: r.author_name,
                                rating: r.rating,
                                body: r.body,
                                published_at: r.published_at,
                            })
                            .await?;
                        synced += 1;
                    }
                }
                Err(e) => warnings.push(format!("Google: {e}")),
            }
        }

        // -- Yelp ------------------------------------------------------------
        if let Some(yelp_id) = &restaurant.yelp_id {
            platforms.push("yelp".into());
            match self.yelp.fetch_reviews(yelp_id).await {
                Ok(fetched) => {
                    for r in fetched {
                        self.reviews
                            .upsert(UpsertReview {
                                restaurant_id,
                                tenant_id: identity.tenant_id,
                                platform: ReviewPlatform::Yelp,
                                external_id: r.external_id,
                                author_name: r.author_name,
                                rating: r.rating,
                                body: r.body,
                                published_at: r.published_at,
                            })
                            .await?;
                        synced += 1;
                    }
                }
                Err(e) => warnings.push(format!("Yelp: {e}")),
            }
        }

        // -- TripAdvisor -----------------------------------------------------
        if let Some(ta_id) = &restaurant.google_place_id {
            // TripAdvisor uses its own location ID stored in a future field;
            // for now we gate on google_place_id being absent to avoid noise.
            // This block is intentionally unreachable until a dedicated
            // `tripadvisor_location_id` column is added in a future migration.
            let _ = ta_id; // suppress unused warning
        }

        Ok(SyncSummary {
            reviews_synced: synced,
            platforms_checked: platforms,
            warnings,
        })
    }

    // -----------------------------------------------------------------------
    // List
    // -----------------------------------------------------------------------

    /// Return a cursor-paginated, filtered list of reviews for a restaurant.
    pub async fn list_reviews(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        query: ReviewQuery,
    ) -> Result<Page<Review>, ReviewError> {
        // Verify tenant ownership.
        let _ = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(ReviewError::RestaurantNotFound(restaurant_id))?;

        let cursor = query.cursor.unwrap_or_else(Cursor::desc_start);
        let limit = query.limit.clamp(1, 100);

        Ok(self
            .reviews
            .list(
                identity.tenant_id,
                restaurant_id,
                ReviewListParams {
                    limit,
                    cursor: Some(cursor),
                    platform: query.platform,
                    min_rating: query.min_rating,
                    from_date: query.from_date,
                    to_date: query.to_date,
                },
            )
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
    use chrono::Utc;

    use forgebike_domain::{
        entities::{
            auth_identity::AuthIdentity,
            restaurant::Restaurant,
            review::{Review, ReviewPlatform},
            user::UserRole,
        },
        identifiers::{RestaurantId, ReviewId, TenantId, UserId},
        pagination::{ListParams, Page},
        ports::{
            restaurant_repository::{NewRestaurant, RestaurantRepository},
            review_fetch_port::{FetchedReview, ReviewFetchPort},
            review_repository::{ReviewListParams, ReviewRepository, UpsertReview},
        },
        DomainError,
    };

    use super::{
        super::{commands::ReviewQuery, error::ReviewError},
        ReviewService,
    };

    // -- Mock: restaurants ---------------------------------------------------

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
            tenant_id: TenantId,
            id: RestaurantId,
        ) -> Result<Option<Restaurant>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id == id && r.tenant_id == tenant_id)
                .cloned())
        }

        async fn list(
            &self,
            tenant_id: TenantId,
            _p: ListParams,
        ) -> Result<Page<Restaurant>, DomainError> {
            Ok(Page {
                items: self
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.tenant_id == tenant_id)
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

    // -- Mock: reviews -------------------------------------------------------

    struct MockReviews(Mutex<Vec<Review>>);

    impl MockReviews {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl ReviewRepository for MockReviews {
        async fn upsert(&self, r: UpsertReview) -> Result<Review, DomainError> {
            let review = Review {
                id: ReviewId::new(),
                restaurant_id: r.restaurant_id,
                tenant_id: r.tenant_id,
                platform: r.platform,
                external_id: r.external_id,
                author_name: r.author_name,
                rating: r.rating,
                body: r.body,
                published_at: r.published_at,
                sentiment_score: None,
                ai_reply_draft: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(review.clone());
            Ok(review)
        }

        async fn list(
            &self,
            tenant_id: TenantId,
            restaurant_id: RestaurantId,
            _p: ReviewListParams,
        ) -> Result<Page<Review>, DomainError> {
            let items: Vec<_> = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.tenant_id == tenant_id && r.restaurant_id == restaurant_id)
                .cloned()
                .collect();
            Ok(Page {
                items,
                next_cursor: None,
            })
        }
    }

    // -- Mock: fetch port ----------------------------------------------------

    struct MockFetchPort(Vec<FetchedReview>);

    impl MockFetchPort {
        fn with(reviews: Vec<FetchedReview>) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(reviews))
        }
        fn empty() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(vec![]))
        }
    }

    #[async_trait]
    impl ReviewFetchPort for MockFetchPort {
        async fn fetch_reviews(&self, _id: &str) -> Result<Vec<FetchedReview>, DomainError> {
            Ok(self.0.clone())
        }
    }

    // -- Helpers -------------------------------------------------------------

    fn make_identity() -> AuthIdentity {
        AuthIdentity {
            user_id: UserId::new(),
            tenant_id: TenantId::new(),
            role: UserRole::Owner,
        }
    }

    fn make_service(
        restaurants: std::sync::Arc<MockRestaurants>,
        reviews: std::sync::Arc<MockReviews>,
        google: std::sync::Arc<MockFetchPort>,
        yelp: std::sync::Arc<MockFetchPort>,
    ) -> ReviewService {
        ReviewService::new(
            reviews as _,
            restaurants as _,
            google as _,
            yelp as _,
            MockFetchPort::empty() as _,
        )
    }

    fn sample_fetched(n: u8) -> FetchedReview {
        FetchedReview {
            external_id: format!("ext-{n}"),
            author_name: format!("Reviewer {n}"),
            rating: 5,
            body: Some(format!("Great! review {n}")),
            published_at: Utc::now(),
        }
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn sync_reviews_with_no_platform_ids_returns_empty_summary() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        // Create a restaurant with NO google_place_id or yelp_id.
        let r = restaurants
            .create(
                forgebike_domain::ports::restaurant_repository::NewRestaurant {
                    tenant_id: identity.tenant_id,
                    name: "Bistro".into(),
                    description: None,
                    cuisine_type: None,
                    address: None,
                    phone: None,
                    website: None,
                },
            )
            .await
            .unwrap();

        let svc = make_service(
            std::sync::Arc::clone(&restaurants),
            std::sync::Arc::clone(&reviews),
            MockFetchPort::empty(),
            MockFetchPort::empty(),
        );
        let summary = svc.sync_reviews(&identity, r.id).await.unwrap();

        assert_eq!(summary.reviews_synced, 0);
        assert!(summary.platforms_checked.is_empty());
        assert!(summary.warnings.is_empty());
    }

    #[tokio::test]
    async fn sync_reviews_calls_google_client_when_place_id_is_set() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        // Manually create a restaurant with a google_place_id.
        let restaurant = Restaurant {
            id: RestaurantId::new(),
            tenant_id: identity.tenant_id,
            name: "Bistro".into(),
            description: None,
            cuisine_type: None,
            address: None,
            phone: None,
            website: None,
            google_place_id: Some("ChIJfake".into()),
            yelp_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        restaurants.0.lock().unwrap().push(restaurant.clone());

        let google = MockFetchPort::with(vec![sample_fetched(1), sample_fetched(2)]);
        let svc = make_service(
            std::sync::Arc::clone(&restaurants),
            std::sync::Arc::clone(&reviews),
            std::sync::Arc::clone(&google),
            MockFetchPort::empty(),
        );

        let summary = svc.sync_reviews(&identity, restaurant.id).await.unwrap();

        assert_eq!(summary.reviews_synced, 2);
        assert_eq!(summary.platforms_checked, vec!["google"]);
        assert_eq!(reviews.0.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn sync_reviews_returns_not_found_for_wrong_tenant() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let owner = make_identity();
        let other = make_identity();

        let r = restaurants
            .create(
                forgebike_domain::ports::restaurant_repository::NewRestaurant {
                    tenant_id: owner.tenant_id,
                    name: "Bistro".into(),
                    description: None,
                    cuisine_type: None,
                    address: None,
                    phone: None,
                    website: None,
                },
            )
            .await
            .unwrap();

        let svc = make_service(
            std::sync::Arc::clone(&restaurants),
            std::sync::Arc::clone(&reviews),
            MockFetchPort::empty(),
            MockFetchPort::empty(),
        );

        let err = svc.sync_reviews(&other, r.id).await.unwrap_err();
        assert!(matches!(err, ReviewError::RestaurantNotFound(_)));
    }

    #[tokio::test]
    async fn list_reviews_returns_only_tenant_reviews() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        let r = restaurants
            .create(
                forgebike_domain::ports::restaurant_repository::NewRestaurant {
                    tenant_id: identity.tenant_id,
                    name: "Bistro".into(),
                    description: None,
                    cuisine_type: None,
                    address: None,
                    phone: None,
                    website: None,
                },
            )
            .await
            .unwrap();

        // Seed a review directly into the repo.
        reviews
            .upsert(UpsertReview {
                restaurant_id: r.id,
                tenant_id: identity.tenant_id,
                platform: ReviewPlatform::Google,
                external_id: "ext-1".into(),
                author_name: "Alice".into(),
                rating: 5,
                body: Some("Lovely!".into()),
                published_at: Utc::now(),
            })
            .await
            .unwrap();

        let svc = make_service(
            std::sync::Arc::clone(&restaurants),
            std::sync::Arc::clone(&reviews),
            MockFetchPort::empty(),
            MockFetchPort::empty(),
        );

        let page = svc
            .list_reviews(
                &identity,
                r.id,
                ReviewQuery {
                    limit: 20,
                    cursor: None,
                    platform: None,
                    min_rating: None,
                    from_date: None,
                    to_date: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].author_name, "Alice");
    }

    #[tokio::test]
    async fn list_reviews_returns_not_found_for_wrong_tenant() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let owner = make_identity();
        let other = make_identity();

        let r = restaurants
            .create(
                forgebike_domain::ports::restaurant_repository::NewRestaurant {
                    tenant_id: owner.tenant_id,
                    name: "Bistro".into(),
                    description: None,
                    cuisine_type: None,
                    address: None,
                    phone: None,
                    website: None,
                },
            )
            .await
            .unwrap();

        let svc = make_service(
            std::sync::Arc::clone(&restaurants),
            std::sync::Arc::clone(&reviews),
            MockFetchPort::empty(),
            MockFetchPort::empty(),
        );

        let err = svc
            .list_reviews(
                &other,
                r.id,
                ReviewQuery {
                    limit: 20,
                    cursor: None,
                    platform: None,
                    min_rating: None,
                    from_date: None,
                    to_date: None,
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ReviewError::RestaurantNotFound(_)));
    }
}
