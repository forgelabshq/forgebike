//! [`AiService`] — sentiment analysis and AI reply draft use cases.

use std::sync::Arc;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, review::Review},
    identifiers::{RestaurantId, ReviewId},
    ports::{
        ai_port::{AiContentPort, ReplyContext},
        restaurant_repository::RestaurantRepository,
        review_repository::ReviewRepository,
        token_usage_store::TokenUsageStore,
    },
};

use super::error::AiError;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Returned by [`AiService::analyse_pending_reviews`].
#[derive(Debug)]
pub struct AnalysisResult {
    /// Number of reviews whose sentiment score was set.
    pub analysed: u32,
    /// Reviews skipped because they had no body text.
    pub skipped: u32,
    /// Total `OpenAI` tokens used across all calls in this batch.
    pub tokens_used: u64,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct AiService {
    reviews: Arc<dyn ReviewRepository>,
    restaurants: Arc<dyn RestaurantRepository>,
    ai_client: Arc<dyn AiContentPort>,
    token_usage: Arc<dyn TokenUsageStore>,
}

impl AiService {
    pub fn new(
        reviews: Arc<dyn ReviewRepository>,
        restaurants: Arc<dyn RestaurantRepository>,
        ai_client: Arc<dyn AiContentPort>,
        token_usage: Arc<dyn TokenUsageStore>,
    ) -> Self {
        Self {
            reviews,
            restaurants,
            ai_client,
            token_usage,
        }
    }

    // -----------------------------------------------------------------------
    // Use cases
    // -----------------------------------------------------------------------

    /// Run sentiment analysis on reviews that do not yet have a score.
    ///
    /// Processes up to 50 reviews per call.  Returns an [`AnalysisResult`]
    /// describing what was done.  If the AI client is not configured (empty
    /// API key) this is a no-op that returns zeroes — callers get a 200 with
    /// `analysed: 0` rather than an error.
    pub async fn analyse_pending_reviews(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
    ) -> Result<AnalysisResult, AiError> {
        // Verify the restaurant belongs to this tenant.
        let _ = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(AiError::RestaurantNotFound(restaurant_id))?;

        let pending = self
            .reviews
            .list_pending_analysis(identity.tenant_id, restaurant_id, 50)
            .await?;

        let mut analysed = 0u32;
        let mut skipped = 0u32;
        let mut tokens_used = 0u64;

        for review in pending {
            let body = match &review.body {
                Some(b) if !b.trim().is_empty() => b.clone(),
                _ => {
                    skipped += 1;
                    continue;
                }
            };

            match self.ai_client.analyse_sentiment(&body).await? {
                Some(result) => {
                    self.reviews
                        .update_sentiment(review.id, result.score)
                        .await?;
                    if result.tokens_used > 0 {
                        let total = self
                            .token_usage
                            .record_usage(identity.tenant_id, result.tokens_used)
                            .await?;
                        tracing::debug!(
                            tenant_id = %identity.tenant_id,
                            tokens    = result.tokens_used,
                            total     = total,
                            "AI tokens recorded"
                        );
                        tokens_used += result.tokens_used;
                    }
                    analysed += 1;
                }
                // AI not configured — skip the entire batch gracefully.
                None => break,
            }
        }

        Ok(AnalysisResult {
            analysed,
            skipped,
            tokens_used,
        })
    }

    /// Fetch a single review, verifying it belongs to the tenant.
    pub async fn get_review(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        review_id: ReviewId,
    ) -> Result<Review, AiError> {
        // Verify restaurant ownership.
        let _ = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(AiError::RestaurantNotFound(restaurant_id))?;

        self.reviews
            .find_by_id(identity.tenant_id, review_id)
            .await?
            .ok_or(AiError::ReviewNotFound(review_id))
    }

    /// Generate an AI reply draft for a review and persist it.
    ///
    /// Returns the draft text. Returns [`AiError::AiUnavailable`] when the
    /// `OpenAI` key is not configured.
    pub async fn generate_reply_draft(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        review_id: ReviewId,
    ) -> Result<String, AiError> {
        // Verify restaurant and review both belong to this tenant.
        let restaurant = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(AiError::RestaurantNotFound(restaurant_id))?;

        let review = self
            .reviews
            .find_by_id(identity.tenant_id, review_id)
            .await?
            .ok_or(AiError::ReviewNotFound(review_id))?;

        let body = review
            .body
            .as_deref()
            .filter(|b| !b.trim().is_empty())
            .ok_or(AiError::NoReviewText(review_id))?;

        let context = ReplyContext {
            review_text: body.to_string(),
            rating: review.rating,
            platform: review.platform.clone(),
            business_name: restaurant.name.clone(),
        };

        let draft = self
            .ai_client
            .generate_reply_draft(&context)
            .await
            .map_err(|e| match e {
                forgebike_domain::DomainError::ExternalService(msg) if msg.contains("API key") => {
                    AiError::AiUnavailable
                }
                other => AiError::Domain(other),
            })?;

        // Persist the draft on the review.
        self.reviews
            .save_reply_draft(review_id, &draft.text)
            .await?;

        // Record token usage.
        if draft.tokens_used > 0 {
            let _ = self
                .token_usage
                .record_usage(identity.tenant_id, draft.tokens_used)
                .await?;
        }

        Ok(draft.text)
    }

    /// Return the total AI tokens used by this tenant in the current month.
    pub async fn get_token_usage(&self, identity: &AuthIdentity) -> Result<u64, AiError> {
        Ok(self
            .token_usage
            .get_monthly_usage(identity.tenant_id)
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
            ai_port::{AiContentPort, ReplyContext, ReplyDraft, SentimentResult},
            restaurant_repository::{NewRestaurant, RestaurantRepository},
            review_repository::{ReviewListParams, ReviewRepository, UpsertReview},
            token_usage_store::TokenUsageStore,
        },
        DomainError,
    };

    use super::{super::error::AiError, AiService};

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

    // -- Mock: ReviewRepository ----------------------------------------------

    struct MockReviews(Mutex<Vec<Review>>);

    impl MockReviews {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }

        fn add(&self, review: Review) {
            self.0.lock().unwrap().push(review);
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
            tid: TenantId,
            rid: RestaurantId,
            _p: ReviewListParams,
        ) -> Result<Page<Review>, DomainError> {
            Ok(Page {
                items: self
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.tenant_id == tid && r.restaurant_id == rid)
                    .cloned()
                    .collect(),
                next_cursor: None,
            })
        }
        async fn find_by_id(
            &self,
            tid: TenantId,
            id: ReviewId,
        ) -> Result<Option<Review>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id == id && r.tenant_id == tid)
                .cloned())
        }
        async fn list_pending_analysis(
            &self,
            tid: TenantId,
            rid: RestaurantId,
            limit: i64,
        ) -> Result<Vec<Review>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|r| {
                    r.tenant_id == tid
                        && r.restaurant_id == rid
                        && r.sentiment_score.is_none()
                        && r.body.as_ref().is_some_and(|b| !b.is_empty())
                })
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .cloned()
                .collect())
        }
        async fn update_sentiment(&self, id: ReviewId, score: f32) -> Result<(), DomainError> {
            let mut guard = self.0.lock().unwrap();
            if let Some(r) = guard.iter_mut().find(|r| r.id == id) {
                r.sentiment_score = Some(score);
            }
            Ok(())
        }
        async fn save_reply_draft(&self, id: ReviewId, draft: &str) -> Result<(), DomainError> {
            let mut guard = self.0.lock().unwrap();
            if let Some(r) = guard.iter_mut().find(|r| r.id == id) {
                r.ai_reply_draft = Some(draft.to_string());
            }
            Ok(())
        }
    }

    // -- Mock: AiContentPort -------------------------------------------------

    struct MockAi {
        configured: bool,
    }

    impl MockAi {
        fn configured() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self { configured: true })
        }
        fn unconfigured() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self { configured: false })
        }
    }

    #[async_trait]
    impl AiContentPort for MockAi {
        async fn analyse_sentiment(
            &self,
            text: &str,
        ) -> Result<Option<SentimentResult>, DomainError> {
            if !self.configured {
                return Ok(None);
            }
            // Simple mock: positive score for text with "great", negative for "terrible"
            let score = if text.contains("great") {
                0.9
            } else if text.contains("terrible") {
                -0.9
            } else {
                0.1
            };
            Ok(Some(SentimentResult {
                score,
                tokens_used: 42,
            }))
        }
        async fn generate_reply_draft(
            &self,
            context: &ReplyContext,
        ) -> Result<ReplyDraft, DomainError> {
            if !self.configured {
                return Err(DomainError::ExternalService(
                    "OpenAI API key is not configured".into(),
                ));
            }
            Ok(ReplyDraft {
                text: format!(
                    "Thank you for your {}-star review of {}!",
                    context.rating, context.business_name
                ),
                tokens_used: 88,
            })
        }
    }

    // -- Mock: TokenUsageStore -----------------------------------------------

    struct MockTokens(Mutex<u64>);

    impl MockTokens {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(0)))
        }
        fn total(&self) -> u64 {
            *self.0.lock().unwrap()
        }
    }

    #[async_trait]
    impl TokenUsageStore for MockTokens {
        async fn record_usage(&self, _tid: TenantId, tokens: u64) -> Result<u64, DomainError> {
            let mut guard = self.0.lock().unwrap();
            *guard += tokens;
            Ok(*guard)
        }
        async fn get_monthly_usage(&self, _tid: TenantId) -> Result<u64, DomainError> {
            Ok(*self.0.lock().unwrap())
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
        ai: std::sync::Arc<MockAi>,
        tokens: std::sync::Arc<MockTokens>,
    ) -> AiService {
        AiService::new(reviews as _, restaurants as _, ai as _, tokens as _)
    }

    fn make_review(restaurant_id: RestaurantId, tenant_id: TenantId, body: Option<&str>) -> Review {
        Review {
            id: ReviewId::new(),
            restaurant_id,
            tenant_id,
            platform: ReviewPlatform::Google,
            external_id: "ext-1".into(),
            author_name: "Alice".into(),
            rating: 5,
            body: body.map(String::from),
            published_at: Utc::now(),
            sentiment_score: None,
            ai_reply_draft: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // -- Tests: analyse_pending_reviews --------------------------------------

    #[tokio::test]
    async fn analyse_pending_scores_reviews_with_body() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let tokens = MockTokens::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        reviews.add(make_review(r.id, identity.tenant_id, Some("great food")));
        reviews.add(make_review(
            r.id,
            identity.tenant_id,
            Some("terrible service"),
        ));

        let svc = make_service(
            restaurants,
            std::sync::Arc::clone(&reviews),
            MockAi::configured(),
            tokens,
        );
        let result = svc.analyse_pending_reviews(&identity, r.id).await.unwrap();

        assert_eq!(result.analysed, 2);
        assert_eq!(result.skipped, 0);
        assert!(result.tokens_used > 0);

        let guard = reviews.0.lock().unwrap();
        assert!(
            guard[0].sentiment_score.is_some(),
            "first review should have a score"
        );
        assert!(
            guard[1].sentiment_score.is_some(),
            "second review should have a score"
        );
    }

    #[tokio::test]
    async fn analyse_pending_skips_reviews_with_no_body() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        // An empty body IS NOT NULL so passes the SQL filter, but the service trims
        // and skips it — this tests the service-level guard.
        reviews.add(make_review(r.id, identity.tenant_id, Some("   ")));

        let svc = make_service(
            restaurants,
            reviews,
            MockAi::configured(),
            MockTokens::new(),
        );
        let result = svc.analyse_pending_reviews(&identity, r.id).await.unwrap();

        assert_eq!(result.analysed, 0);
        assert_eq!(result.skipped, 1);
    }

    #[tokio::test]
    async fn analyse_pending_returns_zero_when_ai_not_configured() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        reviews.add(make_review(r.id, identity.tenant_id, Some("great food")));

        let svc = make_service(
            restaurants,
            reviews,
            MockAi::unconfigured(),
            MockTokens::new(),
        );
        let result = svc.analyse_pending_reviews(&identity, r.id).await.unwrap();

        assert_eq!(
            result.analysed, 0,
            "should be a no-op when AI is unconfigured"
        );
    }

    #[tokio::test]
    async fn analyse_pending_returns_not_found_for_wrong_tenant() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let owner = make_identity();
        let other = make_identity();

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

        let svc = make_service(
            restaurants,
            reviews,
            MockAi::configured(),
            MockTokens::new(),
        );
        let err = svc.analyse_pending_reviews(&other, r.id).await.unwrap_err();
        assert!(matches!(err, AiError::RestaurantNotFound(_)));
    }

    // -- Tests: generate_reply_draft -----------------------------------------

    #[tokio::test]
    async fn generate_reply_draft_saves_draft_and_returns_text() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let tokens = MockTokens::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let review = make_review(r.id, identity.tenant_id, Some("great food"));
        let rid = review.id;
        reviews.add(review);

        let svc = make_service(
            restaurants,
            std::sync::Arc::clone(&reviews),
            MockAi::configured(),
            tokens,
        );
        let draft = svc
            .generate_reply_draft(&identity, r.id, rid)
            .await
            .unwrap();

        assert!(!draft.is_empty(), "draft should not be empty");
        // Check it was persisted
        let guard = reviews.0.lock().unwrap();
        assert_eq!(guard[0].ai_reply_draft.as_deref(), Some(draft.as_str()));
    }

    #[tokio::test]
    async fn generate_reply_draft_returns_ai_unavailable_when_not_configured() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let review = make_review(r.id, identity.tenant_id, Some("some review"));
        let rid = review.id;
        reviews.add(review);

        let svc = make_service(
            restaurants,
            reviews,
            MockAi::unconfigured(),
            MockTokens::new(),
        );
        let err = svc
            .generate_reply_draft(&identity, r.id, rid)
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::AiUnavailable));
    }

    #[tokio::test]
    async fn generate_reply_draft_returns_error_for_review_with_no_body() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let review = make_review(r.id, identity.tenant_id, None); // no body
        let rid = review.id;
        reviews.add(review);

        let svc = make_service(
            restaurants,
            reviews,
            MockAi::configured(),
            MockTokens::new(),
        );
        let err = svc
            .generate_reply_draft(&identity, r.id, rid)
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::NoReviewText(_)));
    }

    // -- Tests: token tracking -----------------------------------------------

    #[tokio::test]
    async fn token_usage_is_recorded_after_analysis() {
        let restaurants = MockRestaurants::new();
        let reviews = MockReviews::new();
        let tokens = MockTokens::new();
        let identity = make_identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        reviews.add(make_review(r.id, identity.tenant_id, Some("great food")));

        let svc = make_service(
            restaurants,
            reviews,
            MockAi::configured(),
            std::sync::Arc::clone(&tokens),
        );
        svc.analyse_pending_reviews(&identity, r.id).await.unwrap();

        assert!(tokens.total() > 0, "tokens should have been recorded");
    }
}
