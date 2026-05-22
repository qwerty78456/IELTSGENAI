//! API configuration and key management

/// Get the Google Gemini API key from environment
///
/// Loads from .env file using dotenvy or environment variables.
/// Safe to use on server.
#[cfg(feature = "server")]
pub fn get_api_key() -> Result<String, String> {
    // Attempt to load .env from current directory, ignore if not found
    let _ = dotenvy::dotenv();

    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("google_api_key"))
        .map_err(|_| "API key not found. Please set GEMINI_API_KEY environment variable or in .env file".to_string())
}

#[cfg(not(feature = "server"))]
#[allow(dead_code)]
pub fn get_api_key() -> Result<String, String> {
    Err("API key is only available on the server".to_string())
}

