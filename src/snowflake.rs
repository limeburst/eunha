// Mastodon-compatible Snowflake ID: 48-bit millisecond timestamp in the
// upper bits, 16 random bits in the lower bits.  Matches Mastodon's Ruby
// implementation exactly (SecureRandom.random_number(0x10000)).
pub fn next_id() -> i64 {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let random = rand::random::<u16>() as u64;
    ((ms << 16) | random) as i64
}
