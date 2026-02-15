use serde::{Deserialize, Serialize};

/// Defines a tool that the LLM can invoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A parsed tool call from the LLM's response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Registry of available tools. Generates descriptions for the system prompt.
pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut tools = Vec::new();

        // Built-in tools
        tools.push(ToolDefinition {
            name: "run_command".to_string(),
            description: "Execute a shell command on the server. Risky commands require admin approval.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        });

        tools.push(ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web for information. Returns a summary of results.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
            }),
        });

        tools.push(ToolDefinition {
            name: "update_persona".to_string(),
            description: "Update a bot persona file (SOUL, IDENTITY, or SECURITY). Admin-only.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_name": {
                        "type": "string",
                        "enum": ["SOUL", "IDENTITY", "SECURITY"],
                        "description": "Which persona file to update"
                    },
                    "new_content": {
                        "type": "string",
                        "description": "The new markdown content for the file"
                    }
                },
                "required": ["file_name", "new_content"]
            }),
        });

        Self { tools }
    }

    /// Generate a human-readable description of all tools for the system prompt.
    pub fn describe_for_prompt(&self) -> String {
        let mut desc = String::from(
            "You have access to the following tools. To use a tool, respond with ONLY a JSON \
             object in the format: {\"tool\": \"tool_name\", \"args\": {...}}\n\n",
        );

        for tool in &self.tools {
            desc.push_str(&format!(
                "- **{}**: {}\n  Parameters: {}\n\n",
                tool.name,
                tool.description,
                serde_json::to_string_pretty(&tool.parameters).unwrap_or_default()
            ));
        }

        desc
    }

    /// Try to parse a tool call from the LLM's text response.
    pub fn parse_tool_call(text: &str) -> Option<ToolCall> {
        // Try to find a JSON object in the response
        let trimmed = text.trim();

        // Look for JSON that starts with { and contains "tool"
        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                let json_str = &trimmed[start..=end];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let (Some(tool), Some(args)) = (
                        val.get("tool").and_then(|t| t.as_str()),
                        val.get("args"),
                    ) {
                        return Some(ToolCall {
                            name: tool.to_string(),
                            arguments: args.clone(),
                        });
                    }
                }
            }
        }

        None
    }
}
