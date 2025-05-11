use anyhow::Result;
use dunce;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportType {
    Stdio,
    Sse { host: String, port: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub root: PathBuf,
    pub ignore_crates: Vec<String>,
}

impl Project {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root_path = root.as_ref();
        
        // First check if the path exists before trying to canonicalize
        if !root_path.exists() {
            return Err(anyhow::anyhow!("Project root path does not exist: {:?}", root_path));
        }
        
        // Use dunce to canonicalize paths on Windows without the \\?\ prefix
        let root = match dunce::canonicalize(root_path) {
            Ok(canonical) => canonical,
            Err(e) => {
                // On Windows, if canonicalize fails but the path exists, use the path as-is
                if cfg!(windows) {
                    tracing::warn!("Failed to canonicalize path, but it exists. Using as-is: {:?}, Error: {}", root_path, e);
                    root_path.to_path_buf()
                } else {
                    // On other platforms, we should be able to canonicalize if it exists
                    return Err(anyhow::anyhow!("Failed to canonicalize project root: {}", e));
                }
            }
        };
        
        Ok(Self {
            root,
            ignore_crates: vec![],
        })
    }

    pub fn ignore_crates(&self) -> &[String] {
        &self.ignore_crates
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn uri(&self) -> Result<Url> {
        Url::from_file_path(&self.root)
            .map_err(|_| anyhow::anyhow!("Failed to create project root URI"))
    }

    pub fn docs_dir(&self) -> PathBuf {
        self.cache_dir().join("doc")
    }

    pub fn cache_folder(&self) -> &str {
        ".docs-cache"
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(self.cache_folder())
    }

    pub fn file_uri(&self, relative_path: impl AsRef<Path>) -> Result<Url> {
        Url::from_file_path(self.root.join(relative_path))
            .map_err(|_| anyhow::anyhow!("Failed to create file URI"))
    }

    /// Given an absolute path, return the path relative to the project root.
    /// Returns an error if the path is not within the project root.
    pub fn relative_path(&self, absolute_path: impl AsRef<Path>) -> Result<String, String> {
        let absolute_path = absolute_path.as_ref();
        
        #[cfg(windows)]
        {
            // Get string representations for comparison
            let root_str = self.root.to_string_lossy();
            let abs_str = absolute_path.to_string_lossy();
            
            // Try various path formats for Windows
            let formats_to_try = [
                // Original paths
                (root_str.to_string(), abs_str.to_string()),
                // Forward slashes
                (root_str.replace('\\', "/"), abs_str.replace('\\', "/")),
                // Backslashes
                (root_str.replace('/', "\\"), abs_str.replace('/', "\\")),
                // Lowercase versions
                (root_str.to_lowercase(), abs_str.to_lowercase()),
                // Lowercase + forward slashes
                (root_str.to_lowercase().replace('\\', "/"), abs_str.to_lowercase().replace('\\', "/")),
                // Lowercase + backslashes
                (root_str.to_lowercase().replace('/', "\\"), abs_str.to_lowercase().replace('/', "\\")),
            ];
            
            for (root_fmt, abs_fmt) in formats_to_try.iter() {
                if abs_fmt.starts_with(root_fmt) {
                    // Calculate the relative path by getting the substring after the root
                    let offset = root_fmt.len();
                    let rel_path = if offset < abs_fmt.len() {
                        let mut rel = abs_fmt[offset..].to_string();
                        // Remove any leading slashes
                        if rel.starts_with('\\') || rel.starts_with('/') {
                            rel = rel[1..].to_string();
                        }
                        rel
                    } else {
                        // If the path is exactly the root, return empty string
                        "".to_string()
                    };
                    
                    tracing::debug!("Windows path resolution: root={}, abs={}, rel={}", 
                                   root_fmt, abs_fmt, rel_path);
                    return Ok(rel_path);
                }
            }
            
            // Advanced logging for debugging path resolution issues
            tracing::warn!("Windows path resolution failed:");
            tracing::warn!("  Project root: {:?}", self.root);
            tracing::warn!("  Absolute path: {:?}", absolute_path);
            for (i, (root_fmt, abs_fmt)) in formats_to_try.iter().enumerate() {
                tracing::warn!("  Attempt {}: {} vs {}", i+1, root_fmt, abs_fmt);
            }
        }
        
        // Non-Windows or fallback path using strip_prefix
        absolute_path
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|_| {
                format!(
                    "Path {:?} is not inside project root {:?}",
                    absolute_path, self.root
                )
            })
    }
}
