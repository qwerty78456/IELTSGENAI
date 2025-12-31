//! API configuration and key management

/// Get the Google Gemini API key from environment
///
/// For native builds: Loads from .env file using dotenvy
/// For WASM builds: Must be set at compile time via GOOGLE_API_KEY env var
pub fn get_api_key() -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Load from .env file for native builds
        dotenvy::from_path(".secrets/api_key.env").ok();
        std::env::var("google_api_key")
            .map_err(|_| "API key not found. Please set google_api_key in .secrets/api_key.env".to_string())
    }

    #[cfg(target_arch = "wasm32")]
    {
        // For WASM, key must be set at compile time
        // This will be embedded in the binary, but at least not in source control
        match option_env!("GOOGLE_API_KEY") {
            Some(key) if !key.is_empty() => Ok(key.to_string()),
            _ => Err("API key not set at compile time. Set GOOGLE_API_KEY environment variable before building.".to_string())
        }
    }
}
