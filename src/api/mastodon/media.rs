use axum::{
    extract::{Extension, Multipart, State},
    Json,
};

use crate::{
    error::{AppError, AppResult},
    media::Storage,
    middleware::AuthenticatedUser,
    state::AppState,
};
use super::{convert::media_from_db, types::MediaAttachment};

// ── POST /api/v2/media ────────────────────────────────────────────────────

pub async fn upload_media(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    mut multipart: Multipart,
) -> AppResult<Json<MediaAttachment>> {
    let mut file_field: Option<(String, String, Vec<u8>)> = None; // (filename, content_type, data)
    let mut description: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::Unprocessable(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("upload").to_string();
                let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
                let data = field.bytes().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                file_field = Some((filename, content_type, data.to_vec()));
            }
            "description" => {
                let text = field.text().await.map_err(|e| AppError::Unprocessable(e.to_string()))?;
                description = Some(text);
            }
            _ => {}
        }
    }

    let (filename, content_type, data) = file_field.ok_or_else(|| AppError::Unprocessable("missing file field".into()))?;
    let media_type = classify_media_type(&content_type);
    let storage = Storage::from_config(&state.config.media_storage);
    let key = storage.store(&data, &filename, &content_type).await?;
    let url = storage.public_url(&key);

    let attachment = sqlx::query_as!(
        crate::db::models::MediaAttachment,
        r#"INSERT INTO media_attachments (account_id, media_type, file_key, file_url, description)
           VALUES ($1,$2,$3,$4,$5)
           RETURNING *"#,
        auth.account_id,
        media_type,
        key,
        url,
        description,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(media_from_db(&attachment)))
}

fn classify_media_type(content_type: &str) -> &'static str {
    if content_type.starts_with("image/gif") {
        "gifv"
    } else if content_type.starts_with("image/") {
        "image"
    } else if content_type.starts_with("video/") {
        "video"
    } else if content_type.starts_with("audio/") {
        "audio"
    } else {
        "unknown"
    }
}
