pub async fn send_confirmation(
    http: &reqwest::Client,
    api_key: &str,
    from: &str,
    to: &str,
    username: &str,
    confirm_url: &str,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "from": from,
        "to": [to],
        "subject": "Confirm your email address",
        "html": format!(
            "<p>Hi @{username},</p>\
             <p>Click the link below to confirm your email address and activate your account.</p>\
             <p><a href=\"{confirm_url}\">{confirm_url}</a></p>"
        ),
    });

    let resp = http
        .post("https://api.resend.com/emails")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Resend API error: {text}");
    }

    Ok(())
}
