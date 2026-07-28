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
