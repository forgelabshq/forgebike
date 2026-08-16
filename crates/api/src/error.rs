//! API error type — maps domain and application errors to HTTP responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use forgebike_application::{
    ai::error::AiError, analytics::error::AnalyticsError, auth::error::AuthError,
    billing::error::BillingError, campaign::error::CampaignError, contact::error::ContactError,
    content::error::ContentError, restaurant::error::RestaurantError, review::error::ReviewError,
};
use forgebike_domain::DomainError;
use serde_json::json;

// ---------------------------------------------------------------------------
// Type alias
// ---------------------------------------------------------------------------

pub type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------
// ApiError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    #[must_use]
    pub fn unauthorised() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthorised")
    }

    #[must_use]
    pub fn unprocessable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::NotFound(msg) => Self::new(StatusCode::NOT_FOUND, msg),
            DomainError::Unauthorised => Self::new(StatusCode::UNAUTHORIZED, "Unauthorised"),
            DomainError::Forbidden => Self::new(StatusCode::FORBIDDEN, "Forbidden"),
            DomainError::Validation(msg) => Self::new(StatusCode::UNPROCESSABLE_ENTITY, msg),
            DomainError::Conflict(msg) => Self::new(StatusCode::CONFLICT, msg),
            DomainError::ExternalService(msg) => {
                tracing::error!(%msg, "External service error");
                Self::new(StatusCode::BAD_GATEWAY, "External service unavailable")
            }
            DomainError::Internal(msg) => {
                tracing::error!(%msg, "Internal error");
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::InvalidCredentials | AuthError::InvalidRefreshToken => {
                Self::new(StatusCode::UNAUTHORIZED, err.to_string())
            }
            AuthError::Domain(domain_err) => Self::from(domain_err),
        }
    }
}

impl From<AiError> for ApiError {
    fn from(err: AiError) -> Self {
        match err {
            AiError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            AiError::ReviewNotFound(id) => Self::not_found(format!("Review {id} not found")),
            AiError::NoReviewText(id) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Review {id} has no text"),
            ),
            AiError::AiUnavailable => Self::new(StatusCode::SERVICE_UNAVAILABLE, err.to_string()),
            AiError::Domain(d) => Self::from(d),
        }
    }
}

impl From<ContentError> for ApiError {
    fn from(err: ContentError) -> Self {
        match err {
            ContentError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            ContentError::ContentNotFound(id) => {
                Self::not_found(format!("Content piece {id} not found"))
            }
            ContentError::AiUnavailable => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, err.to_string())
            }
            ContentError::Domain(d) => Self::from(d),
        }
    }
}

impl From<ReviewError> for ApiError {
    fn from(err: ReviewError) -> Self {
        match err {
            ReviewError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            ReviewError::Domain(domain_err) => Self::from(domain_err),
        }
    }
}

impl From<AnalyticsError> for ApiError {
    fn from(err: AnalyticsError) -> Self {
        match err {
            AnalyticsError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            AnalyticsError::InvalidPeriod(p) => Self::unprocessable(format!(
                "Invalid period: {p} days. Accepted values: 30, 90, 365"
            )),
            AnalyticsError::Domain(d) => Self::from(d),
        }
    }
}

impl From<RestaurantError> for ApiError {
    fn from(err: RestaurantError) -> Self {
        match err {
            RestaurantError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            RestaurantError::MenuItemNotFound(id) => {
                Self::not_found(format!("Menu item {id} not found"))
            }
            RestaurantError::WrongRestaurant(item_id, restaurant_id) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Menu item {item_id} does not belong to restaurant {restaurant_id}"),
            ),
            RestaurantError::Domain(domain_err) => Self::from(domain_err),
        }
    }
}

impl From<ContactError> for ApiError {
    fn from(err: ContactError) -> Self {
        match err {
            ContactError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            ContactError::ContactNotFound(id) => Self::not_found(format!("Contact {id} not found")),
            ContactError::Domain(d) => Self::from(d),
        }
    }
}

impl From<BillingError> for ApiError {
    fn from(err: BillingError) -> Self {
        match err {
            BillingError::InvalidSignature(msg) => Self::new(
                StatusCode::BAD_REQUEST,
                format!("Invalid webhook signature: {msg}"),
            ),
            BillingError::CustomerNotFound(id) => {
                Self::not_found(format!("Stripe customer {id} not found"))
            }
            BillingError::TenantNotFound(id) => Self::not_found(format!("Tenant {id} not found")),
            BillingError::BudgetExceeded { used, limit } => Self::new(
                StatusCode::PAYMENT_REQUIRED,
                format!("Monthly AI token budget exceeded: {used}/{limit} tokens used"),
            ),
            BillingError::FeatureNotAvailable { plan } => Self::new(
                StatusCode::PAYMENT_REQUIRED,
                format!("Feature not available on the {plan} plan — upgrade to unlock"),
            ),
            BillingError::Forbidden => {
                Self::new(StatusCode::FORBIDDEN, "Valid X-Admin-Key header required")
            }
            BillingError::ParseError(msg) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Event parse error: {msg}"),
            ),
            BillingError::Domain(d) => Self::from(d),
        }
    }
}

impl From<CampaignError> for ApiError {
    fn from(err: CampaignError) -> Self {
        match err {
            CampaignError::RestaurantNotFound(id) => {
                Self::not_found(format!("Restaurant {id} not found"))
            }
            CampaignError::CampaignNotFound(id) => {
                Self::not_found(format!("Campaign {id} not found"))
            }
            CampaignError::NotDraft(id) => Self::new(
                StatusCode::CONFLICT,
                format!("Campaign {id} cannot be modified — it is not in draft status"),
            ),
            CampaignError::EmailNotConfigured => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Email is not configured — set APP__EMAIL__SMTP_HOST",
            ),
            CampaignError::SmsNotAvailable => Self::new(
                StatusCode::NOT_IMPLEMENTED,
                "SMS sending is not yet available",
            ),
            CampaignError::Domain(d) => Self::from(d),
        }
    }
}
