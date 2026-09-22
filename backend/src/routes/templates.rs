use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::Serialize;

use crate::auth::MaybeAuthUser;
use crate::db::blocking;
use crate::db::models::Template;
use crate::db::schema::templates;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub built_in: bool,
    #[schema(value_type = crate::domain::templates::TemplateStructure)]
    pub structure: serde_json::Value,
}

impl TryFrom<Template> for TemplateView {
    type Error = ApiError;

    fn try_from(row: Template) -> Result<Self, Self::Error> {
        Ok(Self {
            built_in: row.owner_id.is_none(),
            structure: serde_json::from_str(&row.structure_json)?,
            id: row.id,
            name: row.name,
            kind: row.kind,
            description: row.description,
        })
    }
}

/// Built-ins plus anything the caller has saved as their own template.
#[utoipa::path(
    get,
    path = "/api/templates",
    tag = "templates",
    responses(
        (status = 200, description = "Success", body = Vec<TemplateView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
) -> ApiResult<Json<Vec<TemplateView>>> {
    let user_id = user.map(|u| u.id);
    let rows = blocking(&state.pool, move |conn| {
        let mut query = templates::table.into_boxed();
        query = match &user_id {
            Some(id) => query.filter(
                templates::owner_id
                    .is_null()
                    .or(templates::owner_id.eq(id.clone())),
            ),
            None => query.filter(templates::owner_id.is_null()),
        };
        Ok(query
            .order(templates::name.asc())
            .select(Template::as_select())
            .load(conn)?)
    })
    .await?;

    rows.into_iter()
        .map(TemplateView::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/api/templates/{template_id}",
    tag = "templates",
    params(("template_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = TemplateView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    Path(template_id): Path<String>,
) -> ApiResult<Json<TemplateView>> {
    let row = blocking(&state.pool, move |conn| {
        templates::table
            .find(&template_id)
            .select(Template::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("template not found".into()))
    })
    .await?;
    Ok(Json(TemplateView::try_from(row)?))
}
