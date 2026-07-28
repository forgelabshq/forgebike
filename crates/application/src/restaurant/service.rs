//! [`RestaurantService`] — all restaurant and menu-item use cases.

use std::sync::Arc;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, menu_item::MenuItem, restaurant::Restaurant},
    identifiers::{MenuItemId, RestaurantId},
    pagination::{ListParams, Page},
    ports::{
        menu_item_repository::{MenuItemRepository, NewMenuItem},
        restaurant_repository::{NewRestaurant, RestaurantRepository},
    },
};

use super::{
    commands::{
        CreateMenuItemCommand, CreateRestaurantCommand, UpdateMenuItemCommand,
        UpdateRestaurantCommand,
    },
    error::RestaurantError,
};

pub struct RestaurantService {
    restaurants: Arc<dyn RestaurantRepository>,
    menu_items: Arc<dyn MenuItemRepository>,
}

impl RestaurantService {
    pub fn new(
        restaurants: Arc<dyn RestaurantRepository>,
        menu_items: Arc<dyn MenuItemRepository>,
    ) -> Self {
        Self {
            restaurants,
            menu_items,
        }
    }

    // -----------------------------------------------------------------------
    // Restaurant use cases
    // -----------------------------------------------------------------------

    pub async fn create_restaurant(
        &self,
        identity: &AuthIdentity,
        cmd: CreateRestaurantCommand,
    ) -> Result<Restaurant, RestaurantError> {
        let restaurant = self
            .restaurants
            .create(NewRestaurant {
                tenant_id: identity.tenant_id,
                name: cmd.name,
                description: cmd.description,
                cuisine_type: cmd.cuisine_type,
                address: cmd.address,
                phone: cmd.phone,
                website: cmd.website,
            })
            .await?;
        Ok(restaurant)
    }

    pub async fn get_restaurant(
        &self,
        identity: &AuthIdentity,
        id: RestaurantId,
    ) -> Result<Restaurant, RestaurantError> {
        self.restaurants
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(RestaurantError::RestaurantNotFound(id))
    }

    pub async fn list_restaurants(
        &self,
        identity: &AuthIdentity,
        params: ListParams,
    ) -> Result<Page<Restaurant>, RestaurantError> {
        // Cap limit at 100 to prevent unbounded queries.
        let params = ListParams {
            limit: params.limit.clamp(1, 100),
            ..params
        };
        Ok(self.restaurants.list(identity.tenant_id, params).await?)
    }

    pub async fn update_restaurant(
        &self,
        identity: &AuthIdentity,
        id: RestaurantId,
        cmd: UpdateRestaurantCommand,
    ) -> Result<Restaurant, RestaurantError> {
        // 1. Fetch — ensures the restaurant exists and belongs to this tenant.
        let existing = self
            .restaurants
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(RestaurantError::RestaurantNotFound(id))?;

        // 2. Merge the patch: None fields keep the current value.
        let updated = Restaurant {
            name: cmd.name.unwrap_or(existing.name),
            description: cmd.description.or(existing.description),
            cuisine_type: cmd.cuisine_type.or(existing.cuisine_type),
            address: cmd.address.or(existing.address),
            phone: cmd.phone.or(existing.phone),
            website: cmd.website.or(existing.website),
            google_place_id: cmd.google_place_id.or(existing.google_place_id),
            yelp_id: cmd.yelp_id.or(existing.yelp_id),
            ..existing
        };

        // 3. Persist.
        Ok(self.restaurants.update(&updated).await?)
    }

    pub async fn delete_restaurant(
        &self,
        identity: &AuthIdentity,
        id: RestaurantId,
    ) -> Result<(), RestaurantError> {
        let deleted = self.restaurants.delete(identity.tenant_id, id).await?;
        if deleted {
            Ok(())
        } else {
            Err(RestaurantError::RestaurantNotFound(id))
        }
    }

    // -----------------------------------------------------------------------
    // Menu item use cases
    // -----------------------------------------------------------------------

    pub async fn create_menu_item(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        cmd: CreateMenuItemCommand,
    ) -> Result<MenuItem, RestaurantError> {
        // Verify the restaurant belongs to this tenant before adding items.
        let _ = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(RestaurantError::RestaurantNotFound(restaurant_id))?;

        let item = self
            .menu_items
            .create(NewMenuItem {
                restaurant_id,
                tenant_id: identity.tenant_id,
                name: cmd.name,
                description: cmd.description,
                price_cents: cmd.price_cents,
                category: cmd.category,
                is_available: cmd.is_available,
            })
            .await?;
        Ok(item)
    }

    pub async fn list_menu_items(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        params: ListParams,
    ) -> Result<Page<MenuItem>, RestaurantError> {
        // Verify tenant ownership of the restaurant first.
        let _ = self
            .restaurants
            .find_by_id(identity.tenant_id, restaurant_id)
            .await?
            .ok_or(RestaurantError::RestaurantNotFound(restaurant_id))?;

        let params = ListParams {
            limit: params.limit.clamp(1, 100),
            ..params
        };
        Ok(self
            .menu_items
            .list(identity.tenant_id, restaurant_id, params)
            .await?)
    }

    pub async fn update_menu_item(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        id: MenuItemId,
        cmd: UpdateMenuItemCommand,
    ) -> Result<MenuItem, RestaurantError> {
        let existing = self
            .menu_items
            .find_by_id(identity.tenant_id, id)
            .await?
            .ok_or(RestaurantError::MenuItemNotFound(id))?;

        // Guard: the item must belong to the requested restaurant.
        if existing.restaurant_id != restaurant_id {
            return Err(RestaurantError::WrongRestaurant(id, restaurant_id));
        }

        let updated = MenuItem {
            name: cmd.name.unwrap_or(existing.name),
            description: cmd.description.or(existing.description),
            price_cents: cmd.price_cents.or(existing.price_cents),
            category: cmd.category.or(existing.category),
            is_available: cmd.is_available.unwrap_or(existing.is_available),
            ..existing
        };

        Ok(self.menu_items.update(&updated).await?)
    }

    pub async fn delete_menu_item(
        &self,
        identity: &AuthIdentity,
        restaurant_id: RestaurantId,
        id: MenuItemId,
    ) -> Result<(), RestaurantError> {
        // Verify the item belongs to the right restaurant before deleting.
        if let Some(item) = self.menu_items.find_by_id(identity.tenant_id, id).await? {
            if item.restaurant_id != restaurant_id {
                return Err(RestaurantError::WrongRestaurant(id, restaurant_id));
            }
        }

        let deleted = self.menu_items.delete(identity.tenant_id, id).await?;
        if deleted {
            Ok(())
        } else {
            Err(RestaurantError::MenuItemNotFound(id))
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
            auth_identity::AuthIdentity, menu_item::MenuItem, restaurant::Restaurant,
            user::UserRole,
        },
        identifiers::{MenuItemId, RestaurantId, TenantId, UserId},
        pagination::{ListParams, Page},
        ports::{
            menu_item_repository::{MenuItemRepository, NewMenuItem},
            restaurant_repository::{NewRestaurant, RestaurantRepository},
        },
        DomainError,
    };

    use super::{
        super::{commands::*, error::RestaurantError},
        RestaurantService,
    };

    // -----------------------------------------------------------------------
    // In-memory mock implementations
    // -----------------------------------------------------------------------

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
            _params: ListParams,
        ) -> Result<Page<Restaurant>, DomainError> {
            let items: Vec<_> = self
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
            let mut guard = self.0.lock().unwrap();
            if let Some(existing) = guard.iter_mut().find(|x| x.id == r.id) {
                *existing = r.clone();
            }
            Ok(r.clone())
        }

        async fn delete(&self, tenant_id: TenantId, id: RestaurantId) -> Result<bool, DomainError> {
            let mut guard = self.0.lock().unwrap();
            let before = guard.len();
            guard.retain(|r| !(r.id == id && r.tenant_id == tenant_id));
            Ok(guard.len() < before)
        }
    }

    struct MockMenuItems(Mutex<Vec<MenuItem>>);

    impl MockMenuItems {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl MenuItemRepository for MockMenuItems {
        async fn create(&self, n: NewMenuItem) -> Result<MenuItem, DomainError> {
            let m = MenuItem {
                id: MenuItemId::new(),
                restaurant_id: n.restaurant_id,
                tenant_id: n.tenant_id,
                name: n.name,
                description: n.description,
                price_cents: n.price_cents,
                category: n.category,
                is_available: n.is_available,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(m.clone());
            Ok(m)
        }

        async fn find_by_id(
            &self,
            tenant_id: TenantId,
            id: MenuItemId,
        ) -> Result<Option<MenuItem>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.id == id && m.tenant_id == tenant_id)
                .cloned())
        }

        async fn list(
            &self,
            tenant_id: TenantId,
            restaurant_id: RestaurantId,
            _params: ListParams,
        ) -> Result<Page<MenuItem>, DomainError> {
            let items: Vec<_> = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.tenant_id == tenant_id && m.restaurant_id == restaurant_id)
                .cloned()
                .collect();
            Ok(Page {
                items,
                next_cursor: None,
            })
        }

        async fn update(&self, m: &MenuItem) -> Result<MenuItem, DomainError> {
            let mut guard = self.0.lock().unwrap();
            if let Some(existing) = guard.iter_mut().find(|x| x.id == m.id) {
                *existing = m.clone();
            }
            Ok(m.clone())
        }

        async fn delete(&self, tenant_id: TenantId, id: MenuItemId) -> Result<bool, DomainError> {
            let mut guard = self.0.lock().unwrap();
            let before = guard.len();
            guard.retain(|m| !(m.id == id && m.tenant_id == tenant_id));
            Ok(guard.len() < before)
        }
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn make_identity() -> AuthIdentity {
        AuthIdentity {
            user_id: UserId::new(),
            tenant_id: TenantId::new(),
            role: UserRole::Owner,
        }
    }

    type Fixture = (
        std::sync::Arc<MockRestaurants>,
        std::sync::Arc<MockMenuItems>,
        RestaurantService,
    );

    fn fixture() -> Fixture {
        let restaurants = MockRestaurants::new();
        let menu_items = MockMenuItems::new();
        let svc = RestaurantService::new(
            std::sync::Arc::clone(&restaurants) as _,
            std::sync::Arc::clone(&menu_items) as _,
        );
        (restaurants, menu_items, svc)
    }

    fn create_cmd(name: &str) -> CreateRestaurantCommand {
        CreateRestaurantCommand {
            name: name.into(),
            description: Some("A great place".into()),
            cuisine_type: Some("Italian".into()),
            address: Some("1 Main St".into()),
            phone: None,
            website: None,
        }
    }

    // -----------------------------------------------------------------------
    // Restaurant CRUD
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_restaurant_is_scoped_to_the_identity_tenant() {
        let (repo, _, svc) = fixture();
        let identity = make_identity();

        svc.create_restaurant(&identity, create_cmd("Bistro"))
            .await
            .unwrap();

        let guard = repo.0.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].tenant_id, identity.tenant_id);
        assert_eq!(guard[0].name, "Bistro");
    }

    #[tokio::test]
    async fn get_restaurant_returns_not_found_for_wrong_tenant() {
        let (_, _, svc) = fixture();
        let owner = make_identity();
        let other = make_identity(); // different tenant_id

        let r = svc
            .create_restaurant(&owner, create_cmd("Bistro"))
            .await
            .unwrap();

        let err = svc.get_restaurant(&other, r.id).await.unwrap_err();
        assert!(matches!(err, RestaurantError::RestaurantNotFound(_)));
    }

    #[tokio::test]
    async fn list_restaurants_is_scoped_to_the_identity_tenant() {
        let (_, _, svc) = fixture();
        let a = make_identity();
        let b = make_identity();

        svc.create_restaurant(&a, create_cmd("A Restaurant"))
            .await
            .unwrap();
        svc.create_restaurant(&b, create_cmd("B Restaurant"))
            .await
            .unwrap();

        let page = svc
            .list_restaurants(&a, ListParams::default())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name, "A Restaurant");
    }

    #[tokio::test]
    async fn update_restaurant_preserves_fields_not_included_in_patch() {
        let (_, _, svc) = fixture();
        let identity = make_identity();

        let original = svc
            .create_restaurant(&identity, create_cmd("Original"))
            .await
            .unwrap();

        let updated = svc
            .update_restaurant(
                &identity,
                original.id,
                UpdateRestaurantCommand {
                    name: Some("Updated Name".into()),
                    // All other fields are None — must be preserved.
                    description: None,
                    cuisine_type: None,
                    address: None,
                    phone: None,
                    website: None,
                    google_place_id: None,
                    yelp_id: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(
            updated.description,
            Some("A great place".into()),
            "description must be preserved"
        );
        assert_eq!(
            updated.cuisine_type,
            Some("Italian".into()),
            "cuisine_type must be preserved"
        );
        assert_eq!(
            updated.address,
            Some("1 Main St".into()),
            "address must be preserved"
        );
    }

    #[tokio::test]
    async fn delete_restaurant_returns_not_found_on_second_call() {
        let (_, _, svc) = fixture();
        let identity = make_identity();
        let r = svc
            .create_restaurant(&identity, create_cmd("Temp"))
            .await
            .unwrap();

        svc.delete_restaurant(&identity, r.id).await.unwrap();

        let err = svc.delete_restaurant(&identity, r.id).await.unwrap_err();
        assert!(matches!(err, RestaurantError::RestaurantNotFound(_)));
    }

    // -----------------------------------------------------------------------
    // Menu items
    // -----------------------------------------------------------------------

    fn item_cmd(name: &str, price: i64) -> CreateMenuItemCommand {
        CreateMenuItemCommand {
            name: name.into(),
            description: None,
            price_cents: Some(price),
            category: Some("Mains".into()),
            is_available: true,
        }
    }

    #[tokio::test]
    async fn create_menu_item_is_rejected_when_restaurant_belongs_to_other_tenant() {
        let (_, _, svc) = fixture();
        let owner = make_identity();
        let thief = make_identity();

        let r = svc
            .create_restaurant(&owner, create_cmd("Bistro"))
            .await
            .unwrap();

        let err = svc
            .create_menu_item(&thief, r.id, item_cmd("Stolen Dish", 100))
            .await
            .unwrap_err();

        assert!(matches!(err, RestaurantError::RestaurantNotFound(_)));
    }

    #[tokio::test]
    async fn update_menu_item_rejects_item_from_different_restaurant() {
        let (_, _, svc) = fixture();
        let identity = make_identity();

        let r1 = svc
            .create_restaurant(&identity, create_cmd("R1"))
            .await
            .unwrap();
        let r2 = svc
            .create_restaurant(&identity, create_cmd("R2"))
            .await
            .unwrap();

        // Create an item under r1.
        let item = svc
            .create_menu_item(&identity, r1.id, item_cmd("Pasta", 1500))
            .await
            .unwrap();

        // Try to update it via r2's endpoint — must be rejected.
        let err = svc
            .update_menu_item(
                &identity,
                r2.id, // wrong restaurant
                item.id,
                UpdateMenuItemCommand {
                    name: Some("Hacked".into()),
                    description: None,
                    price_cents: None,
                    category: None,
                    is_available: None,
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(err, RestaurantError::WrongRestaurant(_, _)));
    }

    #[tokio::test]
    async fn update_menu_item_merges_only_supplied_fields() {
        let (_, _, svc) = fixture();
        let identity = make_identity();
        let r = svc
            .create_restaurant(&identity, create_cmd("Bistro"))
            .await
            .unwrap();

        let item = svc
            .create_menu_item(&identity, r.id, item_cmd("Pasta", 1500))
            .await
            .unwrap();

        let updated = svc
            .update_menu_item(
                &identity,
                r.id,
                item.id,
                UpdateMenuItemCommand {
                    name: Some("Tagliatelle".into()),
                    description: None, // unchanged
                    price_cents: None, // unchanged — must stay 1500
                    category: None,    // unchanged
                    is_available: Some(false),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Tagliatelle");
        assert_eq!(updated.price_cents, Some(1500), "price must be preserved");
        assert_eq!(updated.is_available, false);
    }

    #[tokio::test]
    async fn delete_menu_item_returns_not_found_on_second_call() {
        let (_, _, svc) = fixture();
        let identity = make_identity();
        let r = svc
            .create_restaurant(&identity, create_cmd("Bistro"))
            .await
            .unwrap();
        let item = svc
            .create_menu_item(&identity, r.id, item_cmd("Soup", 500))
            .await
            .unwrap();

        svc.delete_menu_item(&identity, r.id, item.id)
            .await
            .unwrap();

        let err = svc
            .delete_menu_item(&identity, r.id, item.id)
            .await
            .unwrap_err();
        assert!(matches!(err, RestaurantError::MenuItemNotFound(_)));
    }
}
