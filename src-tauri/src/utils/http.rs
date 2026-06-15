use once_cell::sync::Lazy;
use reqwest::Client;

/// Global shared HTTP client (15s timeout)
pub static SHARED_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("Failed to build HTTP client")
});

/// Global shared HTTP client (Long timeout: 60s)
pub static SHARED_CLIENT_LONG: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("Failed to build HTTP client")
});

/// Get uniformly configured HTTP client (15s timeout)
pub fn get_client() -> Client {
    SHARED_CLIENT.clone()
}

/// Get long timeout HTTP client (60s timeout)
pub fn get_long_client() -> Client {
    SHARED_CLIENT_LONG.clone()
}

/// Get standard HTTP client (15s timeout, same as get_client)
pub fn get_standard_client() -> Client {
    SHARED_CLIENT.clone()
}

/// Get long timeout standard HTTP client (60s timeout)
pub fn get_long_standard_client() -> Client {
    SHARED_CLIENT_LONG.clone()
}
