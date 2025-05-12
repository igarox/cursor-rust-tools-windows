use std::sync::Arc;

use crate::context::{Context, ProjectContext};
use anyhow::Result;
use mcp_core::{
    tools::ToolHandlerFn,
    types::{CallToolRequest, CallToolResponse, Tool, ToolResponseContent},
};
use serde_json::json;

use super::{
    McpNotification,
    utils::{error_response, get_info_from_request},
};

pub struct CargoCheck;

impl CargoCheck {
    pub fn tool() -> Tool {
        Tool {
            name: "cargo_check".to_string(),
            description: Some(
                "Run the cargo check command in this project. Returns the response in JSON format"
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "The absolute path to the `Cargo.toml` file of the project to check"
                    },
                    "only_errors": {
                        "type": "boolean",
                        "description": "If true, only errors will be returned. If false, errors and warnings will be returned."
                    }
                },
                "required": ["file", "only_errors"]
            }),
        }
    }

    pub fn call(context: Context) -> ToolHandlerFn {
        Box::new(move |request: CallToolRequest| {
            let clone = context.clone();
            Box::pin(async move {
                let (project, relative_file, absolute_file) =
                    match get_info_from_request(&clone, &request).await {
                        Ok(info) => info,
                        Err(response) => return response,
                    };
                if let Err(e) = clone
                    .send_mcp_notification(McpNotification::Request {
                        content: request.clone(),
                        project: absolute_file.clone(),
                    })
                    .await
                {
                    tracing::error!("Failed to send MCP notification: {}", e);
                }
                let response = match handle_request(project, &relative_file, &request).await {
                    Ok(response) => response,
                    Err(response) => response,
                };
                if let Err(e) = clone
                    .send_mcp_notification(McpNotification::Response {
                        content: response.clone(),
                        project: absolute_file.clone(),
                    })
                    .await
                {
                    tracing::error!("Failed to send MCP notification: {}", e);
                }
                response
            })
        })
    }
}

async fn handle_request(
    project: Arc<ProjectContext>,
    _relative_file: &str,
    request: &CallToolRequest,
) -> Result<CallToolResponse, CallToolResponse> {
    let only_errors = request
        .arguments
        .as_ref()
        .and_then(|args| args.get("only_errors"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let project_root = project.project.root().to_string_lossy();
    tracing::info!("Attempting cargo check on project at: {}", project_root);
    
    let messages = match project.cargo_remote.check(only_errors).await {
        Ok(messages) => messages,
        Err(e) => {
            tracing::error!("Cargo check failed: {:?}", e);
            
            #[cfg(windows)]
            let error_message = format!(
                "Cargo check failed: {:?}. This could be due to path resolution issues on Windows. \
                 Please ensure the project is properly registered and the path is accessible.", e
            );
            
            #[cfg(not(windows))]
            let error_message = format!("Cargo check failed: {:?}", e);
            
            return Err(error_response(&error_message));
        }
    };

    let response_message = match serde_json::to_string_pretty(&messages) {
        Ok(message) => message,
        Err(e) => {
            return Err(error_response(&format!("Failed to serialize cargo check results: {:?}", e)));
        }
    };

    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text {
            text: response_message,
        }],
        is_error: None,
        meta: None,
    })
}
