//! [`CampaignService`] — campaign CRUD and send use cases.

use std::sync::Arc;

use chrono::Utc;

use forgebike_domain::{
    entities::{
        auth_identity::AuthIdentity,
        campaign::{Campaign, CampaignChannel, CampaignStatus},
    },
    identifiers::{CampaignId, RestaurantId},
    pagination::Page,
    ports::{
        campaign_repository::{
            CampaignListParams, CampaignRepository, NewCampaign, UpdateCampaign,
        },
        customer_contact_repository::CustomerContactRepository,
        email_port::EmailPort,
        restaurant_repository::RestaurantRepository,
    },
};

use super::error::CampaignError;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Returned by [`CampaignService::send`] once the background task is queued.
#[derive(Debug)]
pub struct SendResult {
    pub campaign_id: CampaignId,
    /// Expected number of recipients (contacts at the time of the call).
    pub recipients_count: i32,
    pub channel: CampaignChannel,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct CampaignService {
    campaigns: Arc<dyn CampaignRepository>,
    contacts: Arc<dyn CustomerContactRepository>,
    restaurants: Arc<dyn RestaurantRepository>,
    email: Arc<dyn EmailPort>,
}

impl CampaignService {
    pub fn new(
        campaigns: Arc<dyn CampaignRepository>,
        contacts: Arc<dyn CustomerContactRepository>,
        restaurants: Arc<dyn RestaurantRepository>,
        email: Arc<dyn EmailPort>,
    ) -> Self {
        Self {
            campaigns,
            contacts,
            restaurants,
            email,
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Confirm the restaurant exists and belongs to this tenant.
    async fn verify_restaurant(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
    ) -> Result<(), CampaignError> {
        self.restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(CampaignError::RestaurantNotFound(restaurant_id))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Use cases
    // -----------------------------------------------------------------------

    /// Create a new campaign in `Draft` status.
    pub async fn create(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        new: NewCampaign,
    ) -> Result<Campaign, CampaignError> {
        self.verify_restaurant(identity, restaurant_id).await?;

        let campaign = self
            .campaigns
            .create(NewCampaign {
                tenant_id: identity.tenant_id,
                restaurant_id,
                ..new
            })
            .await?;

        Ok(campaign)
    }

    /// Fetch a single campaign by ID, scoped to the tenant.
    pub async fn get(
        &self,
        identity: &AuthIdentity,
        id: CampaignId,
    ) -> Result<Campaign, CampaignError> {
        self.campaigns
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(CampaignError::CampaignNotFound(id))
    }

    /// Paginated list of campaigns for a restaurant.
    pub async fn list(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        params: CampaignListParams,
    ) -> Result<Page<Campaign>, CampaignError> {
        self.verify_restaurant(identity, restaurant_id).await?;
        Ok(self
            .campaigns
            .list(identity.tenant_id, restaurant_id, params)
            .await?)
    }

    /// Partial update of a campaign.
    ///
    /// Returns [`CampaignError::NotDraft`] if the campaign has already been
    /// sent or is currently sending — only `Draft` campaigns may be edited.
    pub async fn update(
        &self,
        identity: &AuthIdentity,
        id: CampaignId,
        update: UpdateCampaign,
    ) -> Result<Campaign, CampaignError> {
        // Verify status before delegating to the repo.
        let campaign = self
            .campaigns
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(CampaignError::CampaignNotFound(id))?;

        if campaign.status != CampaignStatus::Draft {
            return Err(CampaignError::NotDraft(id));
        }

        self.campaigns
            .update(identity.tenant_id, id, update)
            .await?
            .ok_or(CampaignError::CampaignNotFound(id))
    }

    /// Delete a campaign.
    ///
    /// Returns [`CampaignError::NotDraft`] when the campaign is not in draft
    /// status; the caller should not be able to delete a campaign mid-send.
    pub async fn delete(
        &self,
        identity: &AuthIdentity,
        id: CampaignId,
    ) -> Result<(), CampaignError> {
        let campaign = self
            .campaigns
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(CampaignError::CampaignNotFound(id))?;

        if campaign.status != CampaignStatus::Draft {
            return Err(CampaignError::NotDraft(id));
        }

        let deleted = self.campaigns.delete(identity.tenant_id, id).await?;
        if deleted {
            Ok(())
        } else {
            Err(CampaignError::CampaignNotFound(id))
        }
    }

    /// Dispatch an email campaign to its filtered contact list.
    ///
    /// The method sets the campaign status to `Sending` and returns
    /// immediately.  A `tokio::spawn`'d background task delivers the emails
    /// and transitions the campaign to `Sent` (with recipient count).
    #[allow(clippy::significant_drop_tightening)]
    pub async fn send(
        &self,
        identity: &AuthIdentity,
        id: CampaignId,
    ) -> Result<SendResult, CampaignError> {
        // 1. Fetch and validate the campaign.
        let campaign = self
            .campaigns
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(CampaignError::CampaignNotFound(id))?;

        // 2. Only draft campaigns can be sent.
        if campaign.status != CampaignStatus::Draft {
            return Err(CampaignError::NotDraft(id));
        }

        // 3. Channel guard.
        if campaign.channel == CampaignChannel::Sms {
            return Err(CampaignError::SmsNotAvailable);
        }

        // 4. Email infrastructure must be configured.
        if !self.email.is_configured() {
            return Err(CampaignError::EmailNotConfigured);
        }

        // 5. Transition to `Sending` synchronously so the UI shows progress.
        self.campaigns
            .set_status(id, CampaignStatus::Sending, None, None)
            .await?;

        // 6. Resolve the recipient list before spawning.
        let recipients = self
            .contacts
            .list_for_campaign(
                identity.tenant_id,
                campaign.restaurant_id,
                campaign.tag_filter.as_deref(),
                None,
            )
            .await?;

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let recipients_count = recipients.len() as i32;
        let channel = campaign.channel.clone();

        // Build owned values for the background task.
        let subject = campaign
            .subject
            .as_deref()
            .unwrap_or(&campaign.name)
            .to_string();
        let body = campaign.body.clone();

        // 7. Clone Arc handles for the spawn.
        let campaigns_bg = Arc::clone(&self.campaigns);
        let email_bg = Arc::clone(&self.email);

        // 8. Fire-and-forget send loop.
        tokio::spawn(async move {
            let mut success_count: i32 = 0;

            for contact in recipients {
                let Some(addr) = contact.email.as_deref().filter(|e| !e.is_empty()) else {
                    continue;
                };
                if let Err(err) = email_bg
                    .send_email(addr, Some(contact.name.as_str()), &subject, &body)
                    .await
                {
                    tracing::error!(
                        campaign_id = %id,
                        contact_email = addr,
                        error = ?err,
                        "failed to send campaign email — skipping recipient"
                    );
                } else {
                    success_count += 1;
                }
            }

            if let Err(err) = campaigns_bg
                .set_status(
                    id,
                    CampaignStatus::Sent,
                    Some(success_count),
                    Some(Utc::now()),
                )
                .await
            {
                tracing::error!(
                    campaign_id = %id,
                    error = ?err,
                    "failed to update campaign status to Sent"
                );
            }
        });

        // 9. Return immediately; the caller knows how many recipients to expect.
        Ok(SendResult {
            campaign_id: id,
            recipients_count,
            channel,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::{DateTime, Utc};

    use forgebike_domain::{
        entities::{
            auth_identity::AuthIdentity,
            campaign::{Campaign, CampaignChannel, CampaignStatus},
            customer_contact::CustomerContact,
            restaurant::Restaurant,
            user::UserRole,
        },
        identifiers::{CampaignId, CustomerContactId, RestaurantId, TenantId, UserId},
        pagination::{ListParams, Page},
        ports::{
            campaign_repository::{
                CampaignListParams, CampaignRepository, NewCampaign, UpdateCampaign,
            },
            customer_contact_repository::{
                ContactListParams, CustomerContactRepository, NewContact, UpdateContact,
            },
            email_port::EmailPort,
            restaurant_repository::{NewRestaurant, RestaurantRepository},
        },
        DomainError,
    };

    use super::{super::error::CampaignError, CampaignService};

    // -- Mock: RestaurantRepository ------------------------------------------

    struct MockRestaurants(Mutex<Vec<Restaurant>>);

    impl MockRestaurants {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(vec![])))
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
            _params: ListParams,
        ) -> Result<Page<Restaurant>, DomainError> {
            let items = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.tenant_id == tenant_id)
                .cloned()
                .collect();
            Ok(Page {
                items,
                next_cursor: None,
            })
        }

        async fn update(&self, r: &Restaurant) -> Result<Restaurant, DomainError> {
            Ok(r.clone())
        }

        async fn delete(&self, tenant_id: TenantId, id: RestaurantId) -> Result<bool, DomainError> {
            let mut guard = self.0.lock().unwrap();
            let before = guard.len();
            guard.retain(|r| !(r.id == id && r.tenant_id == tenant_id));
            Ok(guard.len() < before)
        }
    }

    // -- Mock: CampaignRepository --------------------------------------------

    struct MockCampaigns(Mutex<Vec<Campaign>>);

    impl MockCampaigns {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(vec![])))
        }

        /// Directly seed a campaign (bypasses service logic — useful for
        /// setting up non-draft states in tests).
        fn add(&self, campaign: Campaign) {
            self.0.lock().unwrap().push(campaign);
        }
    }

    #[async_trait]
    impl CampaignRepository for MockCampaigns {
        async fn create(&self, n: NewCampaign) -> Result<Campaign, DomainError> {
            let c = Campaign {
                id: CampaignId::new(),
                tenant_id: n.tenant_id,
                restaurant_id: n.restaurant_id,
                name: n.name,
                channel: n.channel,
                status: CampaignStatus::Draft,
                subject: n.subject,
                body: n.body,
                tag_filter: n.tag_filter,
                scheduled_at: n.scheduled_at,
                sent_at: None,
                recipients_count: 0,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(c.clone());
            Ok(c)
        }

        async fn find_by_id(
            &self,
            tenant_id: TenantId,
            id: CampaignId,
        ) -> Result<Option<Campaign>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id && c.tenant_id == tenant_id)
                .cloned())
        }

        async fn list(
            &self,
            tenant_id: TenantId,
            restaurant_id: RestaurantId,
            _params: CampaignListParams,
        ) -> Result<Page<Campaign>, DomainError> {
            let items = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.tenant_id == tenant_id && c.restaurant_id == restaurant_id)
                .cloned()
                .collect();
            Ok(Page {
                items,
                next_cursor: None,
            })
        }

        async fn update(
            &self,
            tenant_id: TenantId,
            id: CampaignId,
            update: UpdateCampaign,
        ) -> Result<Option<Campaign>, DomainError> {
            let mut guard = self.0.lock().unwrap();
            if let Some(c) = guard
                .iter_mut()
                .find(|c| c.id == id && c.tenant_id == tenant_id)
            {
                if let Some(name) = update.name {
                    c.name = name;
                }
                if let Some(subject) = update.subject {
                    c.subject = subject;
                }
                if let Some(body) = update.body {
                    c.body = body;
                }
                if let Some(tag_filter) = update.tag_filter {
                    c.tag_filter = tag_filter;
                }
                if let Some(scheduled_at) = update.scheduled_at {
                    c.scheduled_at = scheduled_at;
                }
                return Ok(Some(c.clone()));
            }
            Ok(None)
        }

        async fn delete(&self, tenant_id: TenantId, id: CampaignId) -> Result<bool, DomainError> {
            let mut guard = self.0.lock().unwrap();
            let before = guard.len();
            guard.retain(|c| !(c.id == id && c.tenant_id == tenant_id));
            Ok(guard.len() < before)
        }

        async fn set_status(
            &self,
            id: CampaignId,
            status: CampaignStatus,
            recipients_count: Option<i32>,
            sent_at: Option<DateTime<Utc>>,
        ) -> Result<(), DomainError> {
            let mut guard = self.0.lock().unwrap();
            if let Some(c) = guard.iter_mut().find(|c| c.id == id) {
                c.status = status;
                if let Some(count) = recipients_count {
                    c.recipients_count = count;
                }
                if let Some(at) = sent_at {
                    c.sent_at = Some(at);
                }
            }
            Ok(())
        }
    }

    // -- Mock: CustomerContactRepository -------------------------------------

    struct MockContacts(Mutex<Vec<CustomerContact>>);

    impl MockContacts {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl CustomerContactRepository for MockContacts {
        async fn create(&self, n: NewContact) -> Result<CustomerContact, DomainError> {
            let c = CustomerContact {
                id: CustomerContactId::new(),
                tenant_id: n.tenant_id,
                restaurant_id: n.restaurant_id,
                name: n.name,
                email: n.email,
                phone: n.phone,
                tags: n.tags,
                notes: n.notes,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(c.clone());
            Ok(c)
        }

        async fn find_by_id(
            &self,
            tenant_id: TenantId,
            id: CustomerContactId,
        ) -> Result<Option<CustomerContact>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id && c.tenant_id == tenant_id)
                .cloned())
        }

        async fn list(
            &self,
            tenant_id: TenantId,
            restaurant_id: RestaurantId,
            _params: ContactListParams,
        ) -> Result<Page<CustomerContact>, DomainError> {
            let items = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.tenant_id == tenant_id && c.restaurant_id == restaurant_id)
                .cloned()
                .collect();
            Ok(Page {
                items,
                next_cursor: None,
            })
        }

        async fn update(
            &self,
            tenant_id: TenantId,
            id: CustomerContactId,
            _update: UpdateContact,
        ) -> Result<Option<CustomerContact>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id && c.tenant_id == tenant_id)
                .cloned())
        }

        async fn delete(
            &self,
            tenant_id: TenantId,
            id: CustomerContactId,
        ) -> Result<bool, DomainError> {
            let mut guard = self.0.lock().unwrap();
            let before = guard.len();
            guard.retain(|c| !(c.id == id && c.tenant_id == tenant_id));
            Ok(guard.len() < before)
        }

        async fn bulk_create(&self, contacts: Vec<NewContact>) -> Result<usize, DomainError> {
            let count = contacts.len();
            self.0
                .lock()
                .unwrap()
                .extend(contacts.into_iter().map(|n| CustomerContact {
                    id: CustomerContactId::new(),
                    tenant_id: n.tenant_id,
                    restaurant_id: n.restaurant_id,
                    name: n.name,
                    email: n.email,
                    phone: n.phone,
                    tags: n.tags,
                    notes: n.notes,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                }));
            Ok(count)
        }

        async fn list_for_campaign(
            &self,
            tenant_id: TenantId,
            restaurant_id: RestaurantId,
            tag_filter: Option<&str>,
            _since: Option<DateTime<Utc>>,
        ) -> Result<Vec<CustomerContact>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|c| {
                    c.tenant_id == tenant_id
                        && c.restaurant_id == restaurant_id
                        && tag_filter.is_none_or(|tag| c.tags.iter().any(|t| t == tag))
                })
                .cloned()
                .collect())
        }
    }

    // -- Mock: EmailPort -----------------------------------------------------

    struct MockEmail {
        configured: bool,
    }

    impl MockEmail {
        fn configured() -> Arc<Self> {
            Arc::new(Self { configured: true })
        }
        fn unconfigured() -> Arc<Self> {
            Arc::new(Self { configured: false })
        }
    }

    #[async_trait]
    impl EmailPort for MockEmail {
        fn is_configured(&self) -> bool {
            self.configured
        }

        async fn send_email(
            &self,
            _to_address: &str,
            _to_name: Option<&str>,
            _subject: &str,
            _body: &str,
        ) -> Result<(), DomainError> {
            Ok(())
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
        campaigns: Arc<MockCampaigns>,
        contacts: Arc<MockContacts>,
        restaurants: Arc<MockRestaurants>,
        email: Arc<MockEmail>,
    ) -> CampaignService {
        CampaignService::new(campaigns as _, contacts as _, restaurants as _, email as _)
    }

    fn make_draft_campaign(tenant_id: TenantId, restaurant_id: RestaurantId) -> Campaign {
        Campaign {
            id: CampaignId::new(),
            tenant_id,
            restaurant_id,
            name: "Summer Promo".into(),
            channel: CampaignChannel::Email,
            status: CampaignStatus::Draft,
            subject: Some("Big savings!".into()),
            body: "Come visit us.".into(),
            tag_filter: None,
            scheduled_at: None,
            sent_at: None,
            recipients_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn create_campaign_ok() {
        let campaigns = MockCampaigns::new();
        let contacts = MockContacts::new();
        let restaurants = MockRestaurants::new();
        let email = MockEmail::configured();
        let svc = make_service(
            Arc::clone(&campaigns),
            Arc::clone(&contacts),
            Arc::clone(&restaurants),
            Arc::clone(&email),
        );

        let identity = make_identity();

        // Seed a restaurant for this tenant.
        let restaurant = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "My Bistro".into(),
                description: None,
                cuisine_type: Some("Italian".into()),
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let campaign = svc
            .create(
                &identity,
                restaurant.id,
                NewCampaign {
                    tenant_id: identity.tenant_id,
                    restaurant_id: restaurant.id,
                    name: "Grand Opening".into(),
                    channel: CampaignChannel::Email,
                    subject: Some("We're open!".into()),
                    body: "Come see us.".into(),
                    tag_filter: None,
                    scheduled_at: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(campaign.tenant_id, identity.tenant_id);
        assert_eq!(campaign.restaurant_id, restaurant.id);
        assert_eq!(campaign.status, CampaignStatus::Draft);
        assert_eq!(campaign.name, "Grand Opening");

        let guard = campaigns.0.lock().unwrap();
        assert_eq!(guard.len(), 1);
    }

    #[tokio::test]
    async fn send_email_not_configured() {
        let campaigns = MockCampaigns::new();
        let contacts = MockContacts::new();
        let restaurants = MockRestaurants::new();
        let email = MockEmail::unconfigured(); // email not set up
        let svc = make_service(
            Arc::clone(&campaigns),
            Arc::clone(&contacts),
            Arc::clone(&restaurants),
            Arc::clone(&email),
        );

        let identity = make_identity();
        let restaurant_id = RestaurantId::new();

        // Seed a Draft/Email campaign directly.
        let draft = make_draft_campaign(identity.tenant_id, restaurant_id);
        let campaign_id = draft.id;
        campaigns.add(draft);

        let err = svc.send(&identity, campaign_id).await.unwrap_err();

        assert!(
            matches!(err, CampaignError::EmailNotConfigured),
            "expected EmailNotConfigured, got {err:?}"
        );
    }

    #[tokio::test]
    async fn send_sms_not_available() {
        let campaigns = MockCampaigns::new();
        let contacts = MockContacts::new();
        let restaurants = MockRestaurants::new();
        let email = MockEmail::configured();
        let svc = make_service(
            Arc::clone(&campaigns),
            Arc::clone(&contacts),
            Arc::clone(&restaurants),
            Arc::clone(&email),
        );

        let identity = make_identity();
        let restaurant_id = RestaurantId::new();

        // Seed a Draft/SMS campaign.
        let mut draft = make_draft_campaign(identity.tenant_id, restaurant_id);
        draft.channel = CampaignChannel::Sms;
        let campaign_id = draft.id;
        campaigns.add(draft);

        let err = svc.send(&identity, campaign_id).await.unwrap_err();

        assert!(
            matches!(err, CampaignError::SmsNotAvailable),
            "expected SmsNotAvailable, got {err:?}"
        );
    }

    #[tokio::test]
    async fn update_non_draft_rejected() {
        let campaigns = MockCampaigns::new();
        let contacts = MockContacts::new();
        let restaurants = MockRestaurants::new();
        let email = MockEmail::configured();
        let svc = make_service(
            Arc::clone(&campaigns),
            Arc::clone(&contacts),
            Arc::clone(&restaurants),
            Arc::clone(&email),
        );

        let identity = make_identity();
        let restaurant_id = RestaurantId::new();

        // Seed a Sending campaign — update should be rejected.
        let mut sending = make_draft_campaign(identity.tenant_id, restaurant_id);
        sending.status = CampaignStatus::Sending;
        let campaign_id = sending.id;
        campaigns.add(sending);

        let err = svc
            .update(&identity, campaign_id, UpdateCampaign::default())
            .await
            .unwrap_err();

        assert!(
            matches!(err, CampaignError::NotDraft(_)),
            "expected NotDraft, got {err:?}"
        );
    }

    #[tokio::test]
    async fn delete_non_draft_rejected() {
        let campaigns = MockCampaigns::new();
        let contacts = MockContacts::new();
        let restaurants = MockRestaurants::new();
        let email = MockEmail::configured();
        let svc = make_service(
            Arc::clone(&campaigns),
            Arc::clone(&contacts),
            Arc::clone(&restaurants),
            Arc::clone(&email),
        );

        let identity = make_identity();
        let restaurant_id = RestaurantId::new();

        // Seed a Sent campaign — delete should be rejected.
        let mut sent = make_draft_campaign(identity.tenant_id, restaurant_id);
        sent.status = CampaignStatus::Sent;
        let campaign_id = sent.id;
        campaigns.add(sent);

        let err = svc.delete(&identity, campaign_id).await.unwrap_err();

        assert!(
            matches!(err, CampaignError::NotDraft(_)),
            "expected NotDraft, got {err:?}"
        );
    }
}
