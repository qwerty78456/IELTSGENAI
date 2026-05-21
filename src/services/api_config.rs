//! API configuration and key management

/// Get the Google Gemini API key from environment
///
/// Loads from .env file using dotenvy or environment variables.
/// Safe to use on server.
#[cfg(feature = "server")]
pub fn get_api_key() -> Result<String, String> {
    // Try multiple locations for the .secrets/api_key.env file:
    // 1. Current working directory (works with `dx serve`)
    // 2. Executable directory (works with release builds)
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join(".secrets").join("api_key.env")),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(".secrets").join("api_key.env"))),
    ];

    let env_path = candidates
        .into_iter()
        .flatten()
        .find(|p| p.exists())
        .ok_or_else(|| {
            let tried: Vec<String> = [
                std::env::current_dir().ok().map(|d| d.join(".secrets").join("api_key.env")),
                std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(".secrets").join("api_key.env"))),
            ].into_iter().flatten().map(|p| format!("{:?}", p)).collect();
            format!("Could not find .secrets/api_key.env in any of: {}", tried.join(", "))
        })?;

    dotenvy::from_path(&env_path)
        .map_err(|e| format!("Failed to load .env file from {:?}: {}", env_path, e))?;

    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("google_api_key"))
        .map_err(|_| format!("API key not found in {:?}. Please set GEMINI_API_KEY", env_path))
}

#[cfg(not(feature = "server"))]
#[allow(dead_code)]
pub fn get_api_key() -> Result<String, String> {
    Err("API key is only available on the server".to_string())
}

