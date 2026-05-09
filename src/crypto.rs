use crate::error::{AppError, AppResult};

pub fn generate_rsa_keypair() -> AppResult<(String, String)> {
    use rsa::RsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;
    use pkcs8::spki::EncodePublicKey;
    use pkcs8::LineEnding;
    use rsa::rand_core::OsRng;

    let priv_key = RsaPrivateKey::new(&mut OsRng, 2048)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("RSA keygen failed: {e}")))?;

    let priv_doc = priv_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("PKCS8 encode failed: {e}")))?;
    let priv_pem = std::str::from_utf8(priv_doc.as_bytes())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("PEM UTF-8: {e}")))?
        .to_string();

    let pub_pem = priv_key
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("SPKI encode failed: {e}")))?;

    Ok((priv_pem, pub_pem))
}

pub fn hash_password(password: &str) -> AppResult<String> {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::{rand_core::OsRng, SaltString};

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hashing failed: {e}")))
}
