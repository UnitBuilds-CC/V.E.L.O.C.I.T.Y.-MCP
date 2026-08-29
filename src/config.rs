//! Configuration management for VELOCITY-MCP server.
//!
//! Supports loading configuration from:
//! - TOML configuration files
//! - Environment variables (with VELOCITY_ prefix)
//! - Runtime configuration updates
//!
//! Configuration priority (highest to lowest):
//! 1. Environment variables
//! 2. Configuration file
//! 3. Default values

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Main server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server mode (stdio, shmem, http)
    #[serde(default = "default_mode")]
    pub mode: String,
    
    /// Shared memory buffer path (for shmem mode)
    #[serde(default = "default_buffer_path")]
    pub buffer_path: String,
    
    /// HTTP server configuration
    #[serde(default)]
    pub http: HttpConfig,
    
    /// C# engine path
    #[serde(default = "default_csharp_path")]
    pub csharp_path: String,
    
    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
    
    /// Feature flags
    #[serde(default)]
    pub features: FeaturesConfig,
}

/// HTTP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    /// HTTP server address
    #[serde(default = "default_http_addr")]
    pub addr: String,
    
    /// API key for authentication (None = no auth)
    pub api_key: Option<String>,
    
    /// Maximum request body size in bytes
    #[serde(default = "default_max_request_size")]
    pub max_request_size: usize,
    
    /// Enable rate limiting
    #[serde(default = "default_enable_rate_limit")]
    pub enable_rate_limit: bool,
    
    /// Allowed CORS origins
    pub cors_origins: Option<Vec<String>>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            addr: default_http_addr(),
            api_key: None,
            max_request_size: default_max_request_size(),
            enable_rate_limit: default_enable_rate_limit(),
            cors_origins: None,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (error, warn, info, debug, trace)
    #[serde(default = "default_log_level")]
    pub level: String,
}

/// Feature flags configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeaturesConfig {
    /// Enable database resources
    #[serde(default)]
    pub database: bool,
    
    /// Enable OAuth2 support
    #[serde(default)]
    pub oauth2: bool,
    
    /// Enable HTTP transport
    #[serde(default)]
    pub http: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            buffer_path: default_buffer_path(),
            http: HttpConfig::default(),
            csharp_path: default_csharp_path(),
            logging: LoggingConfig::default(),
            features: FeaturesConfig::default(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

fn default_mode() -> String {
    "stdio".to_string()
}

fn default_buffer_path() -> String {
    "nmcp_buffer.bin".to_string()
}

fn default_csharp_path() -> String {
    r"C:\Users\visse\OneDrive\Documents\Payment and Transaction Flow\Velocity\NdaMcpServer\bin\Debug\net10.0\NdaMcpServer.exe".to_string()
}

fn default_http_addr() -> String {
    "0.0.0.0:3000".to_string()
}

fn default_max_request_size() -> usize {
    10 * 1024 * 1024 // 10MB
}

fn default_enable_rate_limit() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

impl ServerConfig {
    /// Load configuration from a TOML file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))
    }
    
    /// Load configuration with environment variable overrides.
    pub fn load_with_env<P: AsRef<Path>>(path: Option<P>) -> Self {
        let mut config = if let Some(path) = path {
            Self::from_file(path).unwrap_or_default()
        } else {
            Self::default()
        };
        
        // Override with environment variables
        if let Ok(mode) = std::env::var("VELOCITY_MODE") {
            config.mode = mode;
        }
        
        if let Ok(buffer_path) = std::env::var("VELOCITY_BUFFER_PATH") {
            config.buffer_path = buffer_path;
        }
        
        if let Ok(csharp_path) = std::env::var("VELOCITY_CSHARP_PATH") {
            config.csharp_path = csharp_path;
        }
        
        if let Ok(log_level) = std::env::var("VELOCITY_LOG_LEVEL") {
            config.logging.level = log_level;
        }
        
        if let Ok(addr) = std::env::var("VELOCITY_HTTP_ADDR") {
            config.http.addr = addr;
        }
        
        if let Ok(api_key) = std::env::var("VELOCITY_API_KEY") {
            config.http.api_key = Some(api_key);
        }
        
        if let Ok(max_size) = std::env::var("VELOCITY_MAX_REQUEST_SIZE") {
            if let Ok(size) = max_size.parse() {
                config.http.max_request_size = size;
            }
        }
        
        if let Ok(enable) = std::env::var("VELOCITY_ENABLE_RATE_LIMIT") {
            config.http.enable_rate_limit = enable.parse().unwrap_or(true);
        }
        
        config
    }
    
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        
        // Validate mode
        if !["stdio", "shmem", "http"].contains(&self.mode.as_str()) {
            errors.push(format!("Invalid mode: {}. Must be stdio, shmem, or http", self.mode));
        }
        
        // Validate log level
        if !["error", "warn", "info", "debug", "trace"].contains(&self.logging.level.as_str()) {
            errors.push(format!("Invalid log level: {}. Must be error, warn, info, debug, or trace", self.logging.level));
        }
        
        // Validate HTTP config
        if self.mode == "http" {
            if self.http.addr.is_empty() {
                errors.push("HTTP address cannot be empty".to_string());
            }
            
            if self.http.max_request_size == 0 {
                errors.push("Max request size must be greater than 0".to_string());
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    
    /// Save configuration to a TOML file.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.mode, "stdio");
        assert_eq!(config.logging.level, "info");
        assert!(config.http.enable_rate_limit);
    }
    
    #[test]
    fn test_config_validation() {
        let mut config = ServerConfig::default();
        assert!(config.validate().is_ok());
        
        config.mode = "invalid".to_string();
        assert!(config.validate().is_err());
        
        config.mode = "http".to_string();
        config.http.addr = "".to_string();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("HTTP address")));
    }
    
    #[test]
    fn test_config_serialization() {
        let config = ServerConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: ServerConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.mode, config.mode);
    }
}
