use rand::{RngCore, thread_rng};

// Generate a secure random token (hex-encoded)
pub fn generate_token(bytes_len: usize) -> String {
    let mut key = vec![0u8; bytes_len];
    thread_rng().fill_bytes(&mut key);
    hex::encode(key)
}
