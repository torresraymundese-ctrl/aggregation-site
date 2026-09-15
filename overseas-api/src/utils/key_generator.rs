use rand::Rng;
use sha2::{Digest, Sha256};

fn random_string(length: usize) -> String {
    let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            charset
                .chars()
                .nth(rng.gen_range(0..charset.len()))
                .unwrap()
        })
        .collect()
}

pub fn generate_api_key(prefix: &str) -> (String, String) {
    let random = random_string(24);
    let full_key = format!("sk-{}-{}", prefix, random);

    let mut hasher = Sha256::new();
    hasher.update(full_key.as_bytes());
    let hash = hex::encode(hasher.finalize());

    (full_key, hash)
}

pub fn get_key_prefix(key: &str) -> String {
    let parts: Vec<&str> = key.split('-').collect();
    if parts.len() >= 2 {
        format!("{}-{}-****", parts[0], parts[1])
    } else {
        "sk-****".to_string()
    }
}
