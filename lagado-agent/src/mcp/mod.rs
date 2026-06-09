//! mcp/mod.rs — Model Context Protocol tool server integration.
//!
//! Phase 1: stub interface. Phase 2: full MCP server + tool discovery.
//! MCP tools merge into operator::core_tools() at runtime.

/// A tool loaded from an MCP server.
#[derive(Debug, Clone)]
pub struct McpTool {
    pub name:        String,
    pub description: String,
    pub server_url:  String,
}

pub struct McpManager {
    tools: Vec<McpTool>,
}

impl McpManager {
    pub fn new() -> Self { Self { tools: Vec::new() } }

    /// Phase 2: connect to configured MCP servers and load their tool manifests.
    pub fn load_tools(&mut self) {
        tracing::info!("MCP tool loading: stub (Phase 2)");
    }

    /// Return all loaded MCP tools as ToolDescriptors for retrieval scoring.
    pub fn as_tool_descriptors(&self) -> Vec<crate::operator::ToolDescriptor> {
        self.tools.iter().map(|t| crate::operator::ToolDescriptor {
            name:        t.name.clone(),
            description: t.description.clone(),
            risk_level:  crate::operator::RiskLevel::Write,
        }).collect()
    }

    pub fn tool_count(&self) -> usize { self.tools.len() }
}
