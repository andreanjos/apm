use rand::{distributions::Alphanumeric, thread_rng, Rng};

pub fn generate_device_code() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

pub fn generate_user_code() -> String {
    let code: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    code.to_ascii_uppercase()
}

pub fn generate_secret_token() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}
