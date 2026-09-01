//! Plugin marketplace for discovering, installing, and managing plugins.
//!
//! The marketplace provides a centralized registry of available plugins with
//! support for versioning, search, and installation management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Plugin metadata for marketplace listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Unique plugin identifier (e.g., "author.plugin-name")
    pub id: String,
    /// Human-readable plugin name
    pub name: String,
    /// Plugin version (semver format)
    pub version: String,
    /// Plugin author
    pub author: String,
    /// Short description
    pub description: String,
    /// Full documentation (markdown)
    #[serde(default)]
    pub documentation: String,
    /// Plugin tags/categories
    #[serde(default)]
    pub tags: Vec<String>,
    /// Download URL for the plugin
    pub download_url: String,
    /// Checksum for verification (SHA-256)
    #[serde(default)]
    pub checksum: String,
    /// Minimum VELOCITY-MCP version required
    #[serde(default)]
    pub min_velocity_version: String,
    /// Plugin dependencies (other plugin IDs)
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Number of downloads
    #[serde(default)]
    pub downloads: u64,
    /// Average rating (0.0-5.0)
    #[serde(default)]
    pub rating: f32,
    /// Number of ratings
    #[serde(default)]
    pub rating_count: u32,
    /// Creation timestamp (ISO 8601)
    #[serde(default)]
    pub created_at: String,
    /// Last update timestamp (ISO 8601)
    #[serde(default)]
    pub updated_at: String,
    /// Whether the plugin is official/verified
    #[serde(default)]
    pub verified: bool,
    /// Plugin reviews
    #[serde(default)]
    pub reviews: Vec<Review>,
}

/// Plugin review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    /// Reviewer username
    pub reviewer: String,
    /// Rating (1-5)
    pub rating: u8,
    /// Review comment
    pub comment: String,
    /// Review timestamp (ISO 8601)
    pub created_at: String,
}

/// Installed plugin information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    /// Plugin metadata
    pub metadata: PluginMetadata,
    /// Installation path
    pub install_path: PathBuf,
    /// Installation timestamp (ISO 8601)
    pub installed_at: String,
    /// Whether the plugin is enabled
    pub enabled: bool,
}

/// Marketplace index containing all available plugins.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketplaceIndex {
    /// All available plugins
    pub plugins: Vec<PluginMetadata>,
    /// Last update timestamp (ISO 8601)
    pub updated_at: String,
    /// Index version
    pub version: String,
}

/// Search query for plugin discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Search text (matches name, description, tags)
    #[serde(default)]
    pub query: String,
    /// Filter by tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Filter by author
    #[serde(default)]
    pub author: Option<String>,
    /// Filter by verified status
    #[serde(default)]
    pub verified_only: bool,
    /// Sort by field (downloads, rating, updated_at)
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    /// Maximum results to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
}

fn default_sort_by() -> String {
    "downloads".to_string()
}

fn default_limit() -> usize {
    20
}

/// Search results from marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Matching plugins
    pub plugins: Vec<PluginMetadata>,
    /// Total number of matches
    pub total: usize,
    /// Current offset
    pub offset: usize,
    /// Limit used
    pub limit: usize,
}

/// Marketplace manager for plugin discovery and installation.
pub struct Marketplace {
    /// Marketplace index
    index: MarketplaceIndex,
    /// Installed plugins
    installed: HashMap<String, InstalledPlugin>,
    /// Storage path for marketplace data
    storage_path: PathBuf,
}

impl Marketplace {
    /// Create a new marketplace manager.
    pub fn new(storage_path: &Path) -> Self {
        let index_path = storage_path.join("index.json");
        let installed_path = storage_path.join("installed.json");
        
        let index = if index_path.exists() {
            match std::fs::read_to_string(&index_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(e) => {
                    warn!(error = %e, "Failed to load marketplace index");
                    MarketplaceIndex::default()
                }
            }
        } else {
            MarketplaceIndex::default()
        };
        
        let installed = if installed_path.exists() {
            match std::fs::read_to_string(&installed_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(e) => {
                    warn!(error = %e, "Failed to load installed plugins");
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };
        
        Self {
            index,
            installed,
            storage_path: storage_path.to_path_buf(),
        }
    }
    
    /// Save marketplace state to disk.
    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.storage_path)
            .map_err(|e| format!("Failed to create marketplace directory: {}", e))?;
        
        let index_path = self.storage_path.join("index.json");
        let index_json = serde_json::to_string_pretty(&self.index)
            .map_err(|e| format!("Failed to serialize index: {}", e))?;
        std::fs::write(&index_path, index_json)
            .map_err(|e| format!("Failed to write index: {}", e))?;
        
        let installed_path = self.storage_path.join("installed.json");
        let installed_json = serde_json::to_string_pretty(&self.installed)
            .map_err(|e| format!("Failed to serialize installed plugins: {}", e))?;
        std::fs::write(&installed_path, installed_json)
            .map_err(|e| format!("Failed to write installed plugins: {}", e))?;
        
        Ok(())
    }
    
    /// Update the marketplace index from a remote source.
    pub fn update_index(&mut self, index: MarketplaceIndex) -> Result<(), String> {
        self.index = index;
        self.save()
    }
    
    /// Search for plugins in the marketplace.
    pub fn search(&self, query: &SearchQuery) -> SearchResults {
        let mut results: Vec<PluginMetadata> = self.index.plugins.iter()
            .filter(|plugin| {
                // Text search
                if !query.query.is_empty() {
                    let query_lower = query.query.to_lowercase();
                    let matches_name = plugin.name.to_lowercase().contains(&query_lower);
                    let matches_desc = plugin.description.to_lowercase().contains(&query_lower);
                    let matches_tags = plugin.tags.iter().any(|t| t.to_lowercase().contains(&query_lower));
                    if !matches_name && !matches_desc && !matches_tags {
                        return false;
                    }
                }
                
                // Tag filter
                if !query.tags.is_empty() {
                    if !query.tags.iter().all(|t| plugin.tags.contains(t)) {
                        return false;
                    }
                }
                
                // Author filter
                if let Some(author) = &query.author {
                    if &plugin.author != author {
                        return false;
                    }
                }
                
                // Verified filter
                if query.verified_only && !plugin.verified {
                    return false;
                }
                
                true
            })
            .cloned()
            .collect();
        
        // Sort results
        match query.sort_by.as_str() {
            "downloads" => results.sort_by(|a, b| b.downloads.cmp(&a.downloads)),
            "rating" => results.sort_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal)),
            "updated_at" => results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
            _ => {}
        }
        
        let total = results.len();
        
        // Apply pagination
        let results = results.into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect();
        
        SearchResults {
            plugins: results,
            total,
            offset: query.offset,
            limit: query.limit,
        }
    }
    
    /// Get a specific plugin by ID.
    pub fn get_plugin(&self, id: &str) -> Option<&PluginMetadata> {
        self.index.plugins.iter().find(|p| p.id == id)
    }
    
    /// Install a plugin from the marketplace.
    pub fn install(&mut self, plugin_id: &str) -> Result<InstalledPlugin, String> {
        // Check if already installed
        if self.installed.contains_key(plugin_id) {
            return Err(format!("Plugin '{}' is already installed", plugin_id));
        }
        
        // Get plugin metadata
        let metadata = self.get_plugin(plugin_id)
            .ok_or_else(|| format!("Plugin '{}' not found in marketplace", plugin_id))?
            .clone();
        
        // Download plugin (simulated for now)
        let install_path = self.storage_path.join("plugins").join(&metadata.id);
        std::fs::create_dir_all(&install_path)
            .map_err(|e| format!("Failed to create plugin directory: {}", e))?;
        
        // In a real implementation, we would download from metadata.download_url
        // For now, we'll just create a placeholder
        let manifest_path = install_path.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
        std::fs::write(&manifest_path, manifest_json)
            .map_err(|e| format!("Failed to write manifest: {}", e))?;
        
        let installed = InstalledPlugin {
            metadata: metadata.clone(),
            install_path,
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: true,
        };
        
        self.installed.insert(plugin_id.to_string(), installed.clone());
        self.save()?;
        
        info!(plugin_id = %plugin_id, "Installed plugin");
        Ok(installed)
    }
    
    /// Uninstall a plugin.
    pub fn uninstall(&mut self, plugin_id: &str) -> Result<(), String> {
        let installed = self.installed.remove(plugin_id)
            .ok_or_else(|| format!("Plugin '{}' is not installed", plugin_id))?;
        
        // Remove plugin directory
        if installed.install_path.exists() {
            std::fs::remove_dir_all(&installed.install_path)
                .map_err(|e| format!("Failed to remove plugin directory: {}", e))?;
        }
        
        self.save()?;
        
        info!(plugin_id = %plugin_id, "Uninstalled plugin");
        Ok(())
    }
    
    /// Get all installed plugins.
    pub fn list_installed(&self) -> Vec<&InstalledPlugin> {
        self.installed.values().collect()
    }
    
    /// Check if a plugin is installed.
    pub fn is_installed(&self, plugin_id: &str) -> bool {
        self.installed.contains_key(plugin_id)
    }
    
    /// Enable or disable an installed plugin.
    pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) -> Result<(), String> {
        let installed = self.installed.get_mut(plugin_id)
            .ok_or_else(|| format!("Plugin '{}' is not installed", plugin_id))?;
        
        installed.enabled = enabled;
        self.save()?;
        
        info!(plugin_id = %plugin_id, enabled = %enabled, "Updated plugin status");
        Ok(())
    }
    
    /// Get marketplace statistics.
    pub fn stats(&self) -> MarketplaceStats {
        MarketplaceStats {
            total_plugins: self.index.plugins.len(),
            installed_plugins: self.installed.len(),
            verified_plugins: self.index.plugins.iter().filter(|p| p.verified).count(),
            total_downloads: self.index.plugins.iter().map(|p| p.downloads).sum(),
        }
    }
    
    /// Submit a review for a plugin.
    pub fn submit_review(&mut self, plugin_id: &str, reviewer: &str, rating: u8, comment: String) -> Result<(), String> {
        // Validate rating
        if rating < 1 || rating > 5 {
            return Err("Rating must be between 1 and 5".to_string());
        }
        
        // Find plugin
        let plugin = self.index.plugins.iter_mut()
            .find(|p| p.id == plugin_id)
            .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;
        
        // Add review
        let review = Review {
            reviewer: reviewer.to_string(),
            rating,
            comment,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        
        plugin.reviews.push(review);
        
        // Update average rating
        plugin.rating_count += 1;
        let total_rating: f32 = plugin.reviews.iter().map(|r| r.rating as f32).sum();
        plugin.rating = total_rating / plugin.rating_count as f32;
        
        self.save()?;
        
        info!(plugin_id = %plugin_id, reviewer = %reviewer, rating = %rating, "Review submitted");
        Ok(())
    }
    
    /// Check for available updates for installed plugins.
    pub fn check_updates(&self) -> Vec<PluginUpdate> {
        let mut updates = Vec::new();
        
        for (plugin_id, installed) in &self.installed {
            if let Some(latest) = self.get_plugin(plugin_id) {
                if latest.version != installed.metadata.version {
                    updates.push(PluginUpdate {
                        plugin_id: plugin_id.clone(),
                        current_version: installed.metadata.version.clone(),
                        latest_version: latest.version.clone(),
                        download_url: latest.download_url.clone(),
                    });
                }
            }
        }
        
        updates
    }
    
    /// Update an installed plugin to the latest version.
    pub fn update_plugin(&mut self, plugin_id: &str) -> Result<InstalledPlugin, String> {
        // Check if installed
        if !self.installed.contains_key(plugin_id) {
            return Err(format!("Plugin '{}' is not installed", plugin_id));
        }
        
        // Uninstall current version
        self.uninstall(plugin_id)?;
        
        // Install latest version
        self.install(plugin_id)
    }
}

/// Plugin update information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginUpdate {
    /// Plugin ID
    pub plugin_id: String,
    /// Current installed version
    pub current_version: String,
    /// Latest available version
    pub latest_version: String,
    /// Download URL for the update
    pub download_url: String,
}

/// Marketplace statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceStats {
    /// Total number of available plugins
    pub total_plugins: usize,
    /// Number of installed plugins
    pub installed_plugins: usize,
    /// Number of verified plugins
    pub verified_plugins: usize,
    /// Total downloads across all plugins
    pub total_downloads: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_timer(name: &str) -> impl Drop {
        let start = std::time::Instant::now();
        struct Timer { name: String, start: std::time::Instant }
        impl Drop for Timer {
            fn drop(&mut self) {
                eprintln!("[TEST] {} completed in {:.3}ms", self.name, self.start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        Timer { name: name.to_string(), start }
    }

    fn make_plugin(id: &str, name: &str, author: &str, tags: Vec<&str>, downloads: u64, rating: f64, verified: bool) -> PluginMetadata {
        PluginMetadata {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            author: author.to_string(),
            description: format!("{} description", name),
            documentation: String::new(),
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            download_url: format!("https://example.com/{}.zip", id),
            checksum: String::new(),
            min_velocity_version: String::new(),
            dependencies: Vec::new(),
            downloads,
            rating: rating as f32,
            rating_count: if rating > 0.0 { 10 } else { 0 },
            created_at: String::new(),
            updated_at: String::new(),
            verified,
            reviews: Vec::new(),
        }
    }

    #[test]
    fn test_marketplace_search() {
        let _t = test_timer("test_marketplace_search");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("test.plugin1", "Test Plugin 1", "Test Author", vec!["test", "example"], 100, 4.5, true));

        let query = SearchQuery {
            query: "test".to_string(),
            tags: Vec::new(),
            author: None,
            verified_only: false,
            sort_by: "downloads".to_string(),
            limit: 10,
            offset: 0,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.total, 1);
        assert_eq!(results.plugins[0].id, "test.plugin1");
    }

    #[test]
    fn test_marketplace_install_uninstall() {
        let _t = test_timer("test_marketplace_install_uninstall");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("test.plugin", "Test Plugin", "Test", vec![], 0, 0.0, false));

        let installed = marketplace.install("test.plugin").unwrap();
        assert_eq!(installed.metadata.id, "test.plugin");
        assert!(marketplace.is_installed("test.plugin"));

        marketplace.uninstall("test.plugin").unwrap();
        assert!(!marketplace.is_installed("test.plugin"));
    }

    #[test]
    fn test_marketplace_search_by_tag() {
        let _t = test_timer("test_marketplace_search_by_tag");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Author", vec!["database", "sql"], 100, 4.0, false));
        marketplace.index.plugins.push(make_plugin("p2", "Plugin 2", "Author", vec!["web", "http"], 200, 4.5, false));
        marketplace.index.plugins.push(make_plugin("p3", "Plugin 3", "Author", vec!["database", "nosql"], 150, 4.2, false));

        let query = SearchQuery {
            query: String::new(),
            tags: vec!["database".to_string()],
            author: None,
            verified_only: false,
            sort_by: "downloads".to_string(),
            limit: 10,
            offset: 0,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.total, 2, "Should find 2 plugins with 'database' tag");
        assert!(results.plugins.iter().all(|p| p.tags.contains(&"database".to_string())));
    }

    #[test]
    fn test_marketplace_search_by_author() {
        let _t = test_timer("test_marketplace_search_by_author");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Alice", vec![], 100, 4.0, false));
        marketplace.index.plugins.push(make_plugin("p2", "Plugin 2", "Bob", vec![], 200, 4.5, false));
        marketplace.index.plugins.push(make_plugin("p3", "Plugin 3", "Alice", vec![], 150, 4.2, false));

        let query = SearchQuery {
            query: String::new(),
            tags: Vec::new(),
            author: Some("Alice".to_string()),
            verified_only: false,
            sort_by: "downloads".to_string(),
            limit: 10,
            offset: 0,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.total, 2, "Should find 2 plugins by Alice");
        assert!(results.plugins.iter().all(|p| p.author == "Alice"));
    }

    #[test]
    fn test_marketplace_search_verified_only() {
        let _t = test_timer("test_marketplace_search_verified_only");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Author", vec![], 100, 4.0, true));
        marketplace.index.plugins.push(make_plugin("p2", "Plugin 2", "Author", vec![], 200, 4.5, false));
        marketplace.index.plugins.push(make_plugin("p3", "Plugin 3", "Author", vec![], 150, 4.2, true));

        let query = SearchQuery {
            query: String::new(),
            tags: Vec::new(),
            author: None,
            verified_only: true,
            sort_by: "downloads".to_string(),
            limit: 10,
            offset: 0,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.total, 2, "Should find 2 verified plugins");
        assert!(results.plugins.iter().all(|p| p.verified));
    }

    #[test]
    fn test_marketplace_search_sort_by_rating() {
        let _t = test_timer("test_marketplace_search_sort_by_rating");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Author", vec![], 100, 3.5, false));
        marketplace.index.plugins.push(make_plugin("p2", "Plugin 2", "Author", vec![], 200, 4.8, false));
        marketplace.index.plugins.push(make_plugin("p3", "Plugin 3", "Author", vec![], 150, 4.2, false));

        let query = SearchQuery {
            query: String::new(),
            tags: Vec::new(),
            author: None,
            verified_only: false,
            sort_by: "rating".to_string(),
            limit: 10,
            offset: 0,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.total, 3);
        assert_eq!(results.plugins[0].id, "p2", "Highest rating should be first");
        assert_eq!(results.plugins[1].id, "p3");
        assert_eq!(results.plugins[2].id, "p1", "Lowest rating should be last");
    }

    #[test]
    fn test_marketplace_search_pagination() {
        let _t = test_timer("test_marketplace_search_pagination");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        for i in 0..10 {
            marketplace.index.plugins.push(make_plugin(&format!("p{}", i), &format!("Plugin {}", i), "Author", vec![], i as u64 * 10, 4.0, false));
        }

        let query = SearchQuery {
            query: String::new(),
            tags: Vec::new(),
            author: None,
            verified_only: false,
            sort_by: "downloads".to_string(),
            limit: 3,
            offset: 0,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.total, 10, "Total should be 10 regardless of pagination");
        assert_eq!(results.plugins.len(), 3, "Should return 3 plugins per page");

        let query = SearchQuery {
            query: String::new(),
            tags: Vec::new(),
            author: None,
            verified_only: false,
            sort_by: "downloads".to_string(),
            limit: 3,
            offset: 3,
        };

        let results = marketplace.search(&query);
        assert_eq!(results.plugins.len(), 3, "Should return 3 plugins for second page");
    }

    #[test]
    fn test_marketplace_submit_review() {
        let _t = test_timer("test_marketplace_submit_review");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Author", vec![], 100, 0.0, false));

        let result = marketplace.submit_review("p1", "Reviewer", 5, "Excellent plugin!".to_string());
        assert!(result.is_ok(), "Review should be submitted successfully: {:?}", result.err());

        let plugin = marketplace.index.plugins.iter().find(|p| p.id == "p1").unwrap();
        assert_eq!(plugin.reviews.len(), 1);
        assert!((plugin.rating - 5.0).abs() < f32::EPSILON, "Rating should be updated to 5.0, got {}", plugin.rating);
        assert_eq!(plugin.rating_count, 1);
    }

    #[test]
    fn test_marketplace_check_updates() {
        let _t = test_timer("test_marketplace_check_updates");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Author", vec![], 100, 4.0, false));
        marketplace.index.plugins[0].version = "2.0.0".to_string();

        marketplace.installed.insert("p1".to_string(), InstalledPlugin {
            metadata: make_plugin("p1", "Plugin 1", "Author", vec![], 100, 4.0, false),
            install_path: PathBuf::from("/tmp/p1"),
            installed_at: "2026-01-01".to_string(),
            enabled: true,
        });

        let updates = marketplace.check_updates();
        assert_eq!(updates.len(), 1, "Should detect 1 available update");
        assert_eq!(updates[0].plugin_id, "p1");
        assert_eq!(updates[0].latest_version, "2.0.0");
    }

    #[test]
    fn test_marketplace_stats() {
        let _t = test_timer("test_marketplace_stats");
        let dir = tempdir().unwrap();
        let mut marketplace = Marketplace::new(dir.path());

        marketplace.index.plugins.push(make_plugin("p1", "Plugin 1", "Author", vec![], 100, 4.0, true));
        marketplace.index.plugins.push(make_plugin("p2", "Plugin 2", "Author", vec![], 200, 4.5, false));
        marketplace.index.plugins.push(make_plugin("p3", "Plugin 3", "Author", vec![], 150, 4.2, true));

        marketplace.installed.insert("p1".to_string(), InstalledPlugin {
            metadata: make_plugin("p1", "Plugin 1", "Author", vec![], 100, 4.0, true),
            install_path: PathBuf::from("/tmp/p1"),
            installed_at: "2026-01-01".to_string(),
            enabled: true,
        });

        let stats = marketplace.stats();
        assert_eq!(stats.total_plugins, 3);
        assert_eq!(stats.installed_plugins, 1);
        assert_eq!(stats.verified_plugins, 2);
        assert_eq!(stats.total_downloads, 450, "Total downloads should be 100 + 200 + 150");
    }
}
