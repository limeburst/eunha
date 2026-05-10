pub async fn send_confirmation(
    http: &reqwest::Client,
    api_key: &str,
    from: &str,
    to: &str,
    name: &str,
    confirm_url: &str,
    locale: &str,
) -> anyhow::Result<()> {
    let (subject, body) = if locale == "ko" {
        (
            "이메일 주소를 인증해 주세요",
            format!(
                "<p>안녕하세요 {name},</p>\
                 <p>아래 링크를 클릭하여 이메일 주소를 인증하고 계정을 활성화해 주세요.</p>\
                 <p><a href=\"{confirm_url}\">{confirm_url}</a></p>"
            ),
        )
    } else {
        (
            "Confirm your email address",
            format!(
                "<p>Hi {name},</p>\
                 <p>Click the link below to confirm your email address and activate your account.</p>\
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
