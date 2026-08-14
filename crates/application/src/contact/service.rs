//! [`ContactService`] — customer-contact use-case orchestration.

use std::sync::Arc;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, customer_contact::CustomerContact},
    identifiers::{CustomerContactId, RestaurantId},
    pagination::Page,
    ports::{
        customer_contact_repository::{
            ContactListParams, CustomerContactRepository, NewContact, UpdateContact,
        },
        restaurant_repository::RestaurantRepository,
    },
};

use super::error::ContactError;

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct ContactService {
    contacts: Arc<dyn CustomerContactRepository>,
    restaurants: Arc<dyn RestaurantRepository>,
}

impl ContactService {
    pub fn new(
        contacts: Arc<dyn CustomerContactRepository>,
        restaurants: Arc<dyn RestaurantRepository>,
    ) -> Self {
        Self {
            contacts,
            restaurants,
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
    ) -> Result<(), ContactError> {
        self.restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(ContactError::RestaurantNotFound(restaurant_id))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Use cases
    // -----------------------------------------------------------------------

    /// Create a single contact for a restaurant.
    ///
    /// Verifies the restaurant exists and belongs to the tenant, then inserts
    /// the contact with the tenant ID from the identity.
    pub async fn create(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        new: NewContact,
    ) -> Result<CustomerContact, ContactError> {
        self.verify_restaurant(identity, restaurant_id).await?;

        let contact = self
            .contacts
            .create(NewContact {
                tenant_id: identity.tenant_id,
                restaurant_id,
                ..new
            })
            .await?;

        Ok(contact)
    }

    /// Fetch a single contact by ID, scoped to the tenant.
    pub async fn get(
        &self,
        identity: &AuthIdentity,
        id: CustomerContactId,
    ) -> Result<CustomerContact, ContactError> {
        self.contacts
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(ContactError::ContactNotFound(id))
    }

    /// Paginated list of contacts for a restaurant.
    pub async fn list(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        params: ContactListParams,
    ) -> Result<Page<CustomerContact>, ContactError> {
        self.verify_restaurant(identity, restaurant_id).await?;
        Ok(self
            .contacts
            .list(identity.tenant_id, restaurant_id, params)
            .await?)
    }

    /// Partial update of an existing contact.
    ///
    /// Returns `ContactNotFound` when the contact does not exist or belongs to
    /// a different tenant.
    pub async fn update(
        &self,
        identity: &AuthIdentity,
        id: CustomerContactId,
        update: UpdateContact,
    ) -> Result<CustomerContact, ContactError> {
        self.contacts
            .update(identity.tenant_id, id, update)
            .await?
            .ok_or(ContactError::ContactNotFound(id))
    }

    /// Delete a contact.  Returns `ContactNotFound` when the contact does not
    /// exist or belongs to a different tenant.
    pub async fn delete(
        &self,
        identity: &AuthIdentity,
        id: CustomerContactId,
    ) -> Result<(), ContactError> {
        let deleted = self.contacts.delete(identity.tenant_id, id).await?;
        if deleted {
            Ok(())
        } else {
            Err(ContactError::ContactNotFound(id))
        }
    }

    /// Bulk-insert contacts.  Skips duplicates on `(tenant_id, restaurant_id,
    /// email)`.  Returns the number of rows actually inserted.
    pub async fn bulk_import(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        contacts: Vec<NewContact>,
    ) -> Result<usize, ContactError> {
        self.verify_restaurant(identity, restaurant_id).await?;

        let with_tenant: Vec<NewContact> = contacts
            .into_iter()
            .map(|c| NewContact {
                tenant_id: identity.tenant_id,
                restaurant_id,
                ..c
            })
            .collect();

        Ok(self.contacts.bulk_create(with_tenant).await?)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;

    use forgebike_domain::{
        entities::{
            auth_identity::AuthIdentity, customer_contact::CustomerContact, restaurant::Restaurant,
            user::UserRole,
        },
        identifiers::{CustomerContactId, RestaurantId, TenantId, UserId},
        pagination::{ListParams, Page},
        ports::{
            customer_contact_repository::{
                ContactListParams, CustomerContactRepository, NewContact, UpdateContact,
            },
            restaurant_repository::{NewRestaurant, RestaurantRepository},
        },
        DomainError,
    };

    use super::{super::error::ContactError, ContactService};

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
            update: UpdateContact,
        ) -> Result<Option<CustomerContact>, DomainError> {
            let mut guard = self.0.lock().unwrap();
            if let Some(c) = guard
                .iter_mut()
                .find(|c| c.id == id && c.tenant_id == tenant_id)
            {
                if let Some(name) = update.name {
                    c.name = name;
                }
                if let Some(email) = update.email {
                    c.email = email;
                }
                if let Some(phone) = update.phone {
                    c.phone = phone;
                }
                if let Some(tags) = update.tags {
                    c.tags = tags;
                }
                if let Some(notes) = update.notes {
                    c.notes = notes;
                }
                return Ok(Some(c.clone()));
            }
            Ok(None)
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
            let mut guard = self.0.lock().unwrap();
            let mut inserted = 0usize;
            for n in contacts {
                // Skip duplicates on (tenant_id, restaurant_id, email).
                let is_dup = n.email.as_ref().is_some_and(|e| {
                    guard.iter().any(|c| {
                        c.tenant_id == n.tenant_id
                            && c.restaurant_id == n.restaurant_id
                            && c.email.as_deref() == Some(e.as_str())
                    })
                });
                if !is_dup {
                    guard.push(CustomerContact {
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
                    });
                    inserted += 1;
                }
            }
            Ok(inserted)
        }

        async fn list_for_campaign(
            &self,
            tenant_id: TenantId,
            restaurant_id: RestaurantId,
            tag_filter: Option<&str>,
            _since: Option<chrono::DateTime<Utc>>,
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

    // -- Helpers -------------------------------------------------------------

    fn make_identity() -> AuthIdentity {
        AuthIdentity {
            user_id: UserId::new(),
            tenant_id: TenantId::new(),
            role: UserRole::Owner,
        }
    }

    type Fixture = (Arc<MockRestaurants>, Arc<MockContacts>, ContactService);

    fn fixture() -> Fixture {
        let restaurants = MockRestaurants::new();
        let contacts = MockContacts::new();
        let svc = ContactService::new(Arc::clone(&contacts) as _, Arc::clone(&restaurants) as _);
        (restaurants, contacts, svc)
    }

    fn new_contact(restaurant_id: RestaurantId) -> NewContact {
        NewContact {
            tenant_id: TenantId::new(), // overwritten by service
            restaurant_id,
            name: "Alice".into(),
            email: Some("alice@example.com".into()),
            phone: None,
            tags: vec!["vip".into()],
            notes: None,
        }
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn create_contact_ok() {
        let (restaurants, contacts, svc) = fixture();
        let identity = make_identity();

        // Seed a restaurant for this tenant.
        let restaurant = restaurants
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

        let contact = svc
            .create(&identity, restaurant.id, new_contact(restaurant.id))
            .await
            .unwrap();

        // The service must stamp the caller's tenant_id regardless of what
        // was inside the NewContact.
        assert_eq!(contact.tenant_id, identity.tenant_id);
        assert_eq!(contact.restaurant_id, restaurant.id);
        assert_eq!(contact.name, "Alice");

        let guard = contacts.0.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].id, contact.id);
    }

    #[tokio::test]
    async fn get_contact_not_found() {
        let (_, _, svc) = fixture();
        let identity = make_identity();

        let err = svc
            .get(&identity, CustomerContactId::new())
            .await
            .unwrap_err();

        assert!(matches!(err, ContactError::ContactNotFound(_)));
    }

    #[tokio::test]
    async fn wrong_tenant_denied() {
        let (restaurants, _, svc) = fixture();

        // A restaurant owned by tenant A.
        let owner = make_identity();
        let restaurant = restaurants
            .create(NewRestaurant {
                tenant_id: owner.tenant_id,
                name: "Owner's Place".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        // A different tenant attempts to create a contact for that restaurant.
        let attacker = make_identity(); // fresh TenantId — not the owner's
        let err = svc
            .create(&attacker, restaurant.id, new_contact(restaurant.id))
            .await
            .unwrap_err();

        assert!(
            matches!(err, ContactError::RestaurantNotFound(_)),
            "expected RestaurantNotFound, got {err:?}"
        );
    }

    // Verify that listing with the wrong cursor type still compiles (exercises
    // the list path with an explicit cursor).
    #[tokio::test]
    async fn list_contacts_scoped_to_restaurant() {
        let (restaurants, _, svc) = fixture();
        let identity = make_identity();

        let r1 = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "R1".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        svc.create(&identity, r1.id, new_contact(r1.id))
            .await
            .unwrap();

        let page = svc
            .list(
                &identity,
                r1.id,
                ContactListParams {
                    limit: 20,
                    cursor: None,
                    tag: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(page.items.len(), 1);
    }

    #[tokio::test]
    async fn bulk_import_returns_count() {
        let (restaurants, _, svc) = fixture();
        let identity = make_identity();

        let restaurant = restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: "Bulk Place".into(),
                description: None,
                cuisine_type: None,
                address: None,
                phone: None,
                website: None,
            })
            .await
            .unwrap();

        let batch: Vec<NewContact> = (0..3)
            .map(|i| NewContact {
                tenant_id: TenantId::new(),
                restaurant_id: restaurant.id,
                name: format!("Contact {i}"),
                email: Some(format!("c{i}@example.com")),
                phone: None,
                tags: vec![],
                notes: None,
            })
            .collect();

        let inserted = svc
            .bulk_import(&identity, restaurant.id, batch)
            .await
            .unwrap();

        assert_eq!(inserted, 3);
    }
}
