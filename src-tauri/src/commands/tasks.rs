use crate::services::project_tasks::{
    self, ProjectTask, RunProjectTaskRequest, RunProjectTaskResult,
};

#[tauri::command]
pub fn discover_project_tasks(path: Option<String>) -> Result<Vec<ProjectTask>, String> {
    project_tasks::discover_project_tasks(path.as_deref())
}

#[tauri::command]
pub async fn run_project_task(
    request: RunProjectTaskRequest,
) -> Result<RunProjectTaskResult, String> {
    project_tasks::run_project_task(request).await
}
