/// Send an account confirmation email.
///
/// `code` — when non-empty, displayed prominently for manual entry (console flow).
///          Leave empty for the Mastodon API flow where only the link is needed.
pub async fn send_confirmation(
    http: &reqwest::Client,
    api_key: &str,
    from: &str,
    to: &str,
    name: &str,
    code: &str,
    confirm_url: &str,
    locale: &str,
) -> anyhow::Result<()> {
    let code_block = if code.is_empty() {
        String::new()
    } else if locale == "ko" {
        format!("<p>인증 코드: <strong style=\"font-size:1.5em;letter-spacing:0.15em\">{code}</strong></p>")
    } else {
        format!("<p>Your confirmation code: <strong style=\"font-size:1.5em;letter-spacing:0.15em\">{code}</strong></p>")
    };

    let (subject, body) = if locale == "ko" {
        (
            "이메일 주소를 인증해 주세요",
            format!(
                "<p>안녕하세요 {name},</p>\
                 {code_block}\
                 <p>또는 아래 링크를 클릭하여 자동으로 인증하세요.</p>\
                 <p><a href=\"{confirm_url}\">{confirm_url}</a></p>"
            ),
        )
    } else {
        (
            "Confirm your email address",
            format!(
                "<p>Hi {name},</p>\
                 {code_block}\
                 <p>Or click the link below to confirm automatically.</p>\
                 <p><a href=\"{confirm_url}\">{confirm_url}</a></p>"
            ),
        )
    };

    let payload = serde_json::json!({
        "from": from,
        "to": [to],
        "subject": subject,
        "html": body,
    });

    let resp = http
        .post("https://api.resend.com/emails")
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Resend API error: {text}");
    }

    Ok(())
}
