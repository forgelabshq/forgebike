//! A combined JSON deserialisation + validation extractor.
//!
//! Using `ValidatedJson<T>` instead of `Json<T>` in a handler signature
//! automatically runs `T: Validate` and returns a `422 Unprocessable Entity`
//! if any field constraint is violated — no boilerplate in the handler body.

use async_trait::async_trait;
use axum::{
    extract::{FromRequest, Request},
    Json,
};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::ApiError;

pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e| ApiError::unprocessable(e.to_string()))?;

        value
            .validate()
            .map_err(|e| ApiError::unprocessable(e.to_string()))?;

        Ok(Self(value))
    }
}
