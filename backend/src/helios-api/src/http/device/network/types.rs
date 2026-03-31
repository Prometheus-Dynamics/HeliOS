use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::http::error::ApiError;

#[derive(Copy, Clone, Debug)]
pub(super) struct TeamNumber(pub(super) u32);

impl TryFrom<u32> for TeamNumber {
    type Error = ApiError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 0 { Err(ApiError::bad_request("team must be greater than zero")) } else { Ok(TeamNumber(value)) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeamNumberPayload {
    pub team_number: Option<u32>,
}
