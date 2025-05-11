use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::context::{Context, ProjectContext};
use anyhow::Result;
use lsp_types::Position;
use mcp_core::types::{CallToolRequest, CallToolResponse, ToolResponseContent};
#[cfg(windows)]
use dunce;

pub fn error_response(message: &str) -> CallToolResponse {
    CallToolResponse {
        content: vec![ToolResponseContent::Text {
            text: message.to_string(),
        }],
        is_error: Some(true),
        meta: None,
    }
}

pub(super) trait RequestExtension {
    fn get_line(&self) -> Result<u64, CallToolResponse>;
    fn get_symbol(&self) -> Result<String, CallToolResponse>;
    fn get_file(&self) -> Result<String, CallToolResponse>;
}

impl RequestExtension for CallToolRequest {
    fn get_line(&self) -> Result<u64, CallToolResponse> {
        let number = self
            .arguments
            .as_ref()
            .and_then(|args| args.get("line"))
            .and_then(|v| v.as_u64())
            .ok_or_else(|| error_response("Line is required"))?;
        // I'm not sure about this. Cursor just now used 0 based indexing
        // Cursor gives llm's line numbers as 1-based, but the LSP uses 0-based
        Ok(number)
        // if number == 0 {
        // return Err(error_response(
        // "Line number must be greater than 0 as line numbers are 1 based",
        // ));
        // }
        // Ok(number - 1)
    }

    fn get_symbol(&self) -> Result<String, CallToolResponse> {
        self.arguments
            .as_ref()
            .and_then(|args| args.get("symbol"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| error_response("Symbol is required"))
            .map(|s| s.to_string())
    }

    fn get_file(&self) -> Result<String, CallToolResponse> {
        self.arguments
            .as_ref()
            .and_then(|args| args.get("file"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| error_response("File is required"))
            .map(|s| s.to_string())
    }
}

/// Returns the project, the relative file path and the absolute file path
pub async fn get_info_from_request(
    context: &Context,
    request: &CallToolRequest,
) -> Result<(Arc<ProjectContext>, String, PathBuf), CallToolResponse> {
    let file = request.get_file()?;
    
    // Normalize Windows paths by replacing backslashes with forward slashes
    #[cfg(windows)]
    let file = file.replace('\\', "/");
    
    tracing::debug!("Processing file path: {}", file);
    let absolute_path = PathBuf::from(file.clone());
    
    // Try to use dunce to get a canonical path without UNC prefixes
    #[cfg(windows)]
    let absolute_path = dunce::canonicalize(&absolute_path)
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to canonicalize path: {}, error: {}", file, e);
            absolute_path
        });
    
    let Some(project) = context.get_project_by_path(&absolute_path).await else {
        #[cfg(windows)]
        {
            // Windows-specific error with helpful information
            tracing::error!("Path format issue on Windows: {}", file);
            return Err(error_response(&format!(
                "No project found for file {}. On Windows, try using forward slashes in paths.",
                file
            )));
        }
        
        #[cfg(not(windows))]
        return Err(error_response(&format!("No project found for file {}", file)));
    };

    let relative_path = match project.project.relative_path(&file) {
        Ok(path) => path,
        Err(e) => {
            #[cfg(windows)]
            {
                tracing::error!("Windows path resolution error: {}", e);
                return Err(error_response(&format!(
                    "{}. Windows paths may need normalization. Try using forward slashes.",
                    e
                )));
            }
            
            #[cfg(not(windows))]
            return Err(error_response(&e));
        }
    };

    Ok((project, relative_path, absolute_path))
}

pub async fn find_symbol_position_in_file(
    project: &Arc<ProjectContext>,
    relative_file: &str,
    symbol: &str,
    line: u64,
) -> Result<Position, String> {
    let symbols = match project.lsp.document_symbols(relative_file).await {
        Ok(Some(symbols)) => symbols,
        Ok(None) => return Err("No symbols found".to_string()),
        Err(e) => return Err(e.to_string()),
    };
    for symbol in symbols {
        if symbol.location.range.start.line == line as u32 {
            return Ok(symbol.location.range.start);
        }
    }
    Err(format!("Symbol {symbol} not found in file {relative_file}"))
}

/// Returns the lines between start_line and end_line (inclusive) from the given file path
/// Optionally includes prefix lines before start_line and suffix lines after end_line
/// Line numbers are 0-based
/// Returns None if any line number is out of bounds after adjusting for prefix/suffix
pub fn get_file_lines(
    file_path: impl AsRef<Path>,
    start_line: u32,
    end_line: u32,
    prefix: u8,
    suffix: u8,
) -> std::io::Result<Option<String>> {
    let content = std::fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();

    // Calculate actual line range accounting for prefix/suffix
    let start = start_line.saturating_sub(prefix as u32);
    let mut end = end_line.saturating_add(suffix as u32);

    if end > lines.len() as u32 {
        end = lines.len() as u32;
    }

    // Check if line range is valid
    if start > end || end >= lines.len() as u32 {
        return Ok(None);
    }

    // Extract and join the requested lines
    let selected_lines = lines[start as usize..=end as usize].join("\n");
    Ok(Some(selected_lines))
}
