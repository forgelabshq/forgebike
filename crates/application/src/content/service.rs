//! [`ContentService`] — AI content generation and management use cases.

use std::sync::Arc;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, content_piece::ContentPiece, restaurant::Restaurant},
    identifiers::{ContentPieceId, RestaurantId},
    pagination::Page,
    ports::{
        ai_port::{AiContentPort, ContentContext},
        content_repository::{ContentListParams, ContentRepository, NewContentPiece},
        restaurant_repository::RestaurantRepository,
        token_usage_store::TokenUsageStore,
    },
};

use super::{
    commands::{ContentListQuery, GenerateContentCommand, UpdateContentCommand},
    error::ContentError,
};

pub struct ContentService {
    content: Arc<dyn ContentRepository>,
    restaurants: Arc<dyn RestaurantRepository>,
    ai_client: Arc<dyn AiContentPort>,
    token_usage: Arc<dyn TokenUsageStore>,
}

impl ContentService {
    pub fn new(
        content: Arc<dyn ContentRepository>,
        restaurants: Arc<dyn RestaurantRepository>,
        ai_client: Arc<dyn AiContentPort>,
        token_usage: Arc<dyn TokenUsageStore>,
    ) -> Self {
        Self {
            content,
            restaurants,
            ai_client,
            token_usage,
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    async fn verify_restaurant(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
    ) -> Result<Restaurant, ContentError> {
        self.restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(ContentError::RestaurantNotFound(restaurant_id))
    }

    fn build_context(restaurant: &Restaurant, cmd: &GenerateContentCommand) -> ContentContext {
        ContentContext {
            content_type: cmd.content_type.clone(),
            business_name: restaurant.name.clone(),
            cuisine_type: restaurant.cuisine_type.clone(),
            topic: cmd.topic.clone(),
            tone: cmd.tone.clone(),
        }
    }

    fn record_tokens(token_usage: Arc<dyn TokenUsageStore>, identity: &AuthIdentity, tokens: u64) {
        if tokens > 0 {
            let identity_tenant_id = identity.tenant_id;
            tokio::spawn(async move {
                let _ = token_usage.record_usage(identity_tenant_id, tokens).await;
            });
        }
    }

    // -----------------------------------------------------------------------
    // Use cases
    // -----------------------------------------------------------------------

    /// Generate marketing content synchronously and store it as a draft.
    pub async fn generate(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        cmd: GenerateContentCommand,
    ) -> Result<ContentPiece, ContentError> {
        let restaurant = self.verify_restaurant(identity, restaurant_id).await?;
        let context = Self::build_context(&restaurant, &cmd);

        let draft = self
            .ai_client
            .generate_content(&context)
            .await
            .map_err(|e| {
                if e.to_string().contains("API key") {
                    ContentError::AiUnavailable
                } else {
                    ContentError::Domain(e)
                }
            })?;

        let piece = self
            .content
            .create(NewContentPiece {
                restaurant_id,
                tenant_id: identity.tenant_id,
                content_type: cmd.content_type,
                title: draft.title,
                body: draft.body,
            })
            .await?;

        Self::record_tokens(Arc::clone(&self.token_usage), identity, draft.tokens_used);

        Ok(piece)
    }

    /// Generate marketing content with **streaming** output.
    ///
    /// Each token chunk is forwarded to `on_chunk` as it arrives.  The
    /// completed piece (saved as a draft) is returned once the stream ends.
    pub async fn stream_generate(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        cmd: GenerateContentCommand,
        on_chunk: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<ContentPiece, ContentError> {
        let restaurant = self.verify_restaurant(identity, restaurant_id).await?;
        let context = Self::build_context(&restaurant, &cmd);

        let draft = self
            .ai_client
            .stream_content(&context, on_chunk)
            .await
            .map_err(|e| {
                if e.to_string().contains("API key") {
                    ContentError::AiUnavailable
                } else {
                    ContentError::Domain(e)
                }
            })?;

        let piece = self
            .content
            .create(NewContentPiece {
                restaurant_id,
                tenant_id: identity.tenant_id,
                content_type: cmd.content_type,
                title: draft.title,
                body: draft.body,
            })
            .await?;

        Self::record_tokens(Arc::clone(&self.token_usage), identity, draft.tokens_used);

        Ok(piece)
    }

    pub async fn list(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        query: ContentListQuery,
    ) -> Result<Page<ContentPiece>, ContentError> {
        let _ = self.verify_restaurant(identity, restaurant_id).await?;

        Ok(self
            .content
            .list(
                identity.tenant_id,
                restaurant_id,
                ContentListParams {
                    limit: query.limit.clamp(1, 100),
                    cursor: query.cursor,
                    status: query.status,
                    content_type: query.content_type,
                },
            )
            .await?)
    }

    pub async fn get(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        content_id: ContentPieceId,
    ) -> Result<ContentPiece, ContentError> {
        let _ = self.verify_restaurant(identity, restaurant_id).await?;
        self.content
            .find_by_id(identity.tenant_id, content_id)
            .await?
            .ok_or(ContentError::ContentNotFound(content_id))
    }

    pub async fn update(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        content_id: ContentPieceId,
        cmd: UpdateContentCommand,
    ) -> Result<ContentPiece, ContentError> {
        let _ = self.verify_restaurant(identity, restaurant_id).await?;

        let existing = self
            .content
            .find_by_id(identity.tenant_id, content_id)
            .await?
            .ok_or(ContentError::ContentNotFound(content_id))?;

        let updated = ContentPiece {
            title: cmd.title.or(existing.title),
            body: cmd.body.unwrap_or(existing.body),
            status: cmd.status.unwrap_or(existing.status),
            ..existing
        };

        Ok(self.content.update(&updated).await?)
    }

    pub async fn delete(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        content_id: ContentPieceId,
    ) -> Result<(), ContentError> {
        let _ = self.verify_restaurant(identity, restaurant_id).await?;
        let deleted = self.content.delete(identity.tenant_id, content_id).await?;
        if deleted {
            Ok(())
        } else {
            Err(ContentError::ContentNotFound(content_id))
        }
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
            content_piece::{ContentPiece, ContentStatus, ContentType},
            restaurant::Restaurant,
            user::UserRole,
        },
        identifiers::{ContentPieceId, RestaurantId, TenantId, UserId},
        pagination::{ListParams, Page},
        ports::{
            ai_port::{
                AiContentPort, ContentContext, ContentDraft, ReplyContext, ReplyDraft,
                SentimentResult,
            },
            content_repository::{ContentListParams, ContentRepository, NewContentPiece},
            restaurant_repository::{NewRestaurant, RestaurantRepository},
            token_usage_store::TokenUsageStore,
        },
        DomainError,
    };

    use super::{
        super::{commands::*, error::ContentError},
        ContentService,
    };

    // -- Mocks ---------------------------------------------------------------

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

    struct MockContent(Mutex<Vec<ContentPiece>>);
    impl MockContent {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl ContentRepository for MockContent {
        async fn create(&self, p: NewContentPiece) -> Result<ContentPiece, DomainError> {
            let piece = ContentPiece {
                id: ContentPieceId::new(),
                restaurant_id: p.restaurant_id,
                tenant_id: p.tenant_id,
                content_type: p.content_type,
                title: p.title,
                body: p.body,
                status: ContentStatus::Draft,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(piece.clone());
            Ok(piece)
        }
        async fn find_by_id(
            &self,
            tid: TenantId,
            id: ContentPieceId,
        ) -> Result<Option<ContentPiece>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == id && p.tenant_id == tid)
                .cloned())
        }
        async fn list(
            &self,
            tid: TenantId,
            rid: RestaurantId,
            p: ContentListParams,
        ) -> Result<Page<ContentPiece>, DomainError> {
            let items: Vec<_> = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.tenant_id == tid && c.restaurant_id == rid)
                .filter(|c| p.status.as_ref().is_none_or(|s| &c.status == s))
                .filter(|c| p.content_type.as_ref().is_none_or(|t| &c.content_type == t))
                .take(usize::try_from(p.limit).unwrap_or(usize::MAX))
                .cloned()
                .collect();
            Ok(Page {
                items,
                next_cursor: None,
            })
        }
        async fn update(&self, piece: &ContentPiece) -> Result<ContentPiece, DomainError> {
            let mut g = self.0.lock().unwrap();
            if let Some(p) = g.iter_mut().find(|p| p.id == piece.id) {
                *p = piece.clone();
            }
            Ok(piece.clone())
        }
        async fn delete(&self, tid: TenantId, id: ContentPieceId) -> Result<bool, DomainError> {
            let mut g = self.0.lock().unwrap();
            let before = g.len();
            g.retain(|p| !(p.id == id && p.tenant_id == tid));
            Ok(g.len() < before)
        }
    }

    struct MockAi {
        configured: bool,
    }
    impl MockAi {
        fn on() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self { configured: true })
        }
        fn off() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self { configured: false })
        }
    }

    #[async_trait]
    impl AiContentPort for MockAi {
        async fn analyse_sentiment(
            &self,
            _t: &str,
        ) -> Result<Option<SentimentResult>, DomainError> {
            Ok(None)
        }
        async fn generate_reply_draft(&self, _c: &ReplyContext) -> Result<ReplyDraft, DomainError> {
            Ok(ReplyDraft {
                text: "reply".into(),
                tokens_used: 10,
            })
        }
        async fn generate_content(
            &self,
            ctx: &ContentContext,
        ) -> Result<ContentDraft, DomainError> {
            if !self.configured {
                return Err(DomainError::ExternalService(
                    "OpenAI API key is not configured".into(),
                ));
            }
            let body = format!(
                "Generated {} content for {}",
                ctx.content_type, ctx.business_name
            );
            Ok(ContentDraft {
                title: None,
                body,
                tokens_used: 50,
            })
        }
        async fn stream_content(
            &self,
            ctx: &ContentContext,
            on_chunk: std::sync::Arc<dyn Fn(String) + Send + Sync>,
        ) -> Result<ContentDraft, DomainError> {
            if !self.configured {
                return Err(DomainError::ExternalService(
                    "OpenAI API key is not configured".into(),
                ));
            }
            let body = format!("Streamed {} content", ctx.content_type);
            on_chunk("Streamed ".into());
            on_chunk(format!("{} content", ctx.content_type));
            Ok(ContentDraft {
                title: None,
                body,
                tokens_used: 40,
            })
        }
    }

    struct MockTokens(Mutex<u64>);
    impl MockTokens {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(0)))
        }
    }
    #[async_trait]
    impl TokenUsageStore for MockTokens {
        async fn record_usage(&self, _tid: TenantId, tokens: u64) -> Result<u64, DomainError> {
            let mut g = self.0.lock().unwrap();
            *g += tokens;
            Ok(*g)
        }
        async fn get_monthly_usage(&self, _tid: TenantId) -> Result<u64, DomainError> {
            Ok(*self.0.lock().unwrap())
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
        content: std::sync::Arc<MockContent>,
        ai: std::sync::Arc<MockAi>,
    ) -> ContentService {
        ContentService::new(
            content as _,
            restaurants as _,
            ai as _,
            MockTokens::new() as _,
        )
    }

    fn gen_cmd(ct: ContentType) -> GenerateContentCommand {
        GenerateContentCommand {
            content_type: ct,
            topic: None,
            tone: None,
        }
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn generate_creates_draft_piece() {
        let restaurants = MockRestaurants::new();
        let content = MockContent::new();
        let id = identity();

        let r = restaurants
            .create(NewRestaurant {
                tenant_id: id.tenant_id,
                name: "Bistro".into(),
                description: None,
                cuisine_type: Some("Italian".into()),
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let s = svc(restaurants, std::sync::Arc::clone(&content), MockAi::on());
        let piece = s
            .generate(&id, r.id, gen_cmd(ContentType::SocialPost))
            .await
            .unwrap();

        assert_eq!(piece.status, ContentStatus::Draft);
        assert_eq!(piece.content_type, ContentType::SocialPost);
        assert!(!piece.body.is_empty());
        assert_eq!(content.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn generate_returns_ai_unavailable_when_not_configured() {
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

        let s = svc(restaurants, MockContent::new(), MockAi::off());
        let err = s
            .generate(&id, r.id, gen_cmd(ContentType::Email))
            .await
            .unwrap_err();
        assert!(matches!(err, ContentError::AiUnavailable));
    }

    #[tokio::test]
    async fn generate_returns_not_found_for_wrong_tenant() {
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

        let s = svc(restaurants, MockContent::new(), MockAi::on());
        let err = s
            .generate(&other, r.id, gen_cmd(ContentType::BlogIntro))
            .await
            .unwrap_err();
        assert!(matches!(err, ContentError::RestaurantNotFound(_)));
    }

    #[tokio::test]
    async fn stream_generate_calls_on_chunk_and_saves() {
        let restaurants = MockRestaurants::new();
        let content = MockContent::new();
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

        let received = std::sync::Arc::new(Mutex::new(vec![]));
        let recv_clone = std::sync::Arc::clone(&received);
        let on_chunk: std::sync::Arc<dyn Fn(String) + Send + Sync> =
            std::sync::Arc::new(move |c| {
                recv_clone.lock().unwrap().push(c);
            });

        let s = svc(restaurants, std::sync::Arc::clone(&content), MockAi::on());
        let piece = s
            .stream_generate(&id, r.id, gen_cmd(ContentType::MenuDescription), on_chunk)
            .await
            .unwrap();

        assert_eq!(piece.status, ContentStatus::Draft);
        assert!(
            !received.lock().unwrap().is_empty(),
            "on_chunk should have been called"
        );
        assert_eq!(content.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_returns_only_tenant_content() {
        let restaurants = MockRestaurants::new();
        let content = MockContent::new();
        let a = identity();
        let b = identity();

        let ra = restaurants
            .create(NewRestaurant {
                tenant_id: a.tenant_id,
                name: "A".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();
        let rb = restaurants
            .create(NewRestaurant {
                tenant_id: b.tenant_id,
                name: "B".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let sa = svc(
            std::sync::Arc::clone(&restaurants),
            std::sync::Arc::clone(&content),
            MockAi::on(),
        );
        let sb = ContentService::new(
            std::sync::Arc::clone(&content) as _,
            std::sync::Arc::clone(&restaurants) as _,
            MockAi::on() as _,
            MockTokens::new() as _,
        );

        sa.generate(&a, ra.id, gen_cmd(ContentType::SocialPost))
            .await
            .unwrap();
        sb.generate(&b, rb.id, gen_cmd(ContentType::Email))
            .await
            .unwrap();

        let page = sa
            .list(
                &a,
                ra.id,
                ContentListQuery {
                    limit: 20,
                    cursor: None,
                    status: None,
                    content_type: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].content_type, ContentType::SocialPost);
    }

    #[tokio::test]
    async fn update_changes_status_and_body() {
        let restaurants = MockRestaurants::new();
        let content = MockContent::new();
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

        let s = svc(restaurants, std::sync::Arc::clone(&content), MockAi::on());
        let piece = s
            .generate(&id, r.id, gen_cmd(ContentType::BlogIntro))
            .await
            .unwrap();

        let updated = s
            .update(
                &id,
                r.id,
                piece.id,
                UpdateContentCommand {
                    title: None,
                    body: Some("Edited body".into()),
                    status: Some(ContentStatus::Approved),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.body, "Edited body");
        assert_eq!(updated.status, ContentStatus::Approved);
    }

    #[tokio::test]
    async fn delete_removes_piece() {
        let restaurants = MockRestaurants::new();
        let content = MockContent::new();
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

        let s = svc(restaurants, std::sync::Arc::clone(&content), MockAi::on());
        let piece = s
            .generate(&id, r.id, gen_cmd(ContentType::SocialPost))
            .await
            .unwrap();

        s.delete(&id, r.id, piece.id).await.unwrap();

        let err = s.delete(&id, r.id, piece.id).await.unwrap_err();
        assert!(matches!(err, ContentError::ContentNotFound(_)));
    }
}
