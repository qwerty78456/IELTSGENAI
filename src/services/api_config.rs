//! API configuration and key management

/// Get the Google Gemini API key from environment
///
/// Loads from .env file using dotenvy or environment variables.
/// Safe to use on server.
#[cfg(feature = "server")]
pub fn get_api_key() -> Result<String, String> {
    // Get the directory where the executable is located
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Failed to get executable path: {}", e))?;
    let exe_dir = exe_path.parent()
        .ok_or("Failed to get executable directory")?;
    
    let env_path = exe_dir.join(".secrets\\api_key.env");
    
    dotenvy::from_path(&env_path)
        .map_err(|e| format!("Failed to load .env file from {:?}: {}", env_path, e))?;
    
    std::env::var("google_api_key")
        .map_err(|_| format!("API key not found in {:?}. Please set google_api_key", env_path))
}

#[cfg(not(feature = "server"))]
pub fn get_api_key() -> Result<String, String> {
    Err("API key is only available on the server".to_string())
}

