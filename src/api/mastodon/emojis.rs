use axum::{extract::State, response::Json};
use crate::{
    error::AppResult,
    state::AppState,
};
use super::types::CustomEmoji;

pub async fn list_custom_emojis(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<CustomEmoji>>> {
    let rows = sqlx::query!(
        r#"SELECT ce.shortcode, ce.image_url, ce.static_image_url, ce.visible_in_picker,
                  ecc.name AS "category_name?"
           FROM custom_emojis ce
           LEFT JOIN custom_emoji_categories ecc ON ecc.id = ce.category_id
           WHERE ce.domain IS NULL
             AND ce.disabled = false
           ORDER BY ce.shortcode"#,
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
            category: r.category_name,
            featured: None,
        })
        .collect();

    Ok(Json(emojis))
}
