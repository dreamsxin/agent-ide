use serde::{Deserialize, Serialize};

/// 传递给 Agent 的上下文
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentContext {
    pub active_file: Option<String>,
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    pub open_files: Vec<String>,
    pub project_path: String,
}

impl AgentContext {
    pub fn new(project_path: &str) -> Self {
        Self {
            active_file: None,
            active_file_content: None,
            selection: None,
            open_files: Vec::new(),
            project_path: project_path.to_string(),
        }
    }

    /// 构建注入到 system prompt 的上下文字符串
    pub fn to_prompt_context(&self) -> String {
        let mut ctx = String::new();
        ctx.push_str("=== Project Context ===\n");
        ctx.push_str(&format!("Project: {}\n", self.project_path));

        if let Some(ref file) = self.active_file {
            ctx.push_str(&format!("Active file: {}\n", file));
        }
        if let Some(ref content) = self.active_file_content {
            ctx.push_str(&format!(
                "Current file content:\n```\n{}\n```\n",
                content
            ));
        }
        if let Some(ref selection) = self.selection {
            ctx.push_str(&format!(
                "Selected code:\n```\n{}\n```\n",
                selection
            ));
        }
        if !self.open_files.is_empty() {
            ctx.push_str(&format!("Open files: {:?}\n", self.open_files));
        }

        ctx
    }
}
