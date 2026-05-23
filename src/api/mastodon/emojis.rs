use axum::{extract::State, response::Json, Extension};
use crate::{
    error::AppResult,
    middleware::ResolvedInstance,
    state::AppState,
};
use super::types::CustomEmoji;

pub async fn list_custom_emojis(
    State(state): State<AppState>,
    Extension(ResolvedInstance(instance)): Extension<ResolvedInstance>,
) -> AppResult<Json<Vec<CustomEmoji>>> {
    let rows = sqlx::query!(
        r#"SELECT shortcode, image_url, static_image_url, visible_in_picker
           FROM custom_emojis
           WHERE instance_id = $1
             AND domain IS NULL
             AND disabled = false
           ORDER BY shortcode"#,
        instance.id,
    )
    .fetch_all(&state.db)
    .await?;

    let emojis = rows
        .into_iter()
        .map(|r| CustomEmoji {
            shortcode: r.shortcode,
            url: r.image_url.clone(),
            static_url: r.static_image_url.unwrap_or(r.image_url),
            visible_in_picker: r.visible_in_picker,
            category: None,
            featured: None,
        })
        .collect();

    Ok(Json(emojis))
}
