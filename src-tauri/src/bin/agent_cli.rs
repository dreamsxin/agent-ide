// ============================================================
// Agent IDE CLI — command-line AI coding agent
// ============================================================
// Usage:
//   agent-cli --endpoint URL --api-key KEY --model NAME "your prompt"
//   agent-cli --apply "your prompt"        (use env vars + write files)
//   or set env: LLM_ENDPOINT, LLM_API_KEY, LLM_MODEL

use agent_ide_lib::agent::planner;
use agent_ide_lib::agent::executor;
use agent_ide_lib::services::llm_client::{LlmClient, LlmConfig};
use agent_ide_lib::services::context::AgentContext;
use tokio::sync::mpsc;
use std::path::PathBuf;
use std::fs;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut endpoint = String::new();
    let mut api_key = String::new();
    let mut model = String::new();
    let mut prompt = String::new();
    let mut workspace = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut apply = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--endpoint" => { i += 1; if i < args.len() { endpoint = args[i].clone(); } }
            "--api-key" => { i += 1; if i < args.len() { api_key = args[i].clone(); } }
            "--model" => { i += 1; if i < args.len() { model = args[i].clone(); } }
            "--workspace" => { i += 1; if i < args.len() { workspace = args[i].clone(); } }
            "--apply" => { apply = true; }
            "--help" | "-h" => { print_help(); return; }
            other if !other.starts_with('-') => {
                if prompt.is_empty() { prompt = other.to_string(); }
            }
            _ => { eprintln!("Unknown flag: {}", args[i]); return; }
        }
        i += 1;
    }

    // Env fallback
    if endpoint.is_empty() { endpoint = std::env::var("LLM_ENDPOINT").unwrap_or_default(); }
    if api_key.is_empty() { api_key = std::env::var("LLM_API_KEY").unwrap_or_default(); }
    if model.is_empty() { model = std::env::var("LLM_MODEL").unwrap_or_default(); }

    if endpoint.is_empty() || api_key.is_empty() || model.is_empty() || prompt.is_empty() {
        eprintln!("Error: --endpoint, --api-key, --model, and prompt are required");
        eprintln!("Or set env vars: LLM_ENDPOINT, LLM_API_KEY, LLM_MODEL");
        return;
    }

    println!("=== Agent IDE CLI ===");
    println!("Endpoint: {}", endpoint);
    println!("Model:    {}", model);
    println!("Workspace:{}", workspace);
    println!("Prompt:   {}", prompt);
    if apply { println!("Mode:     Apply (files will be written)"); }
    else { println!("Mode:     Preview only"); }
    println!();

    let config = LlmConfig { endpoint, api_key, model };
    let llm = LlmClient::new(config);

    let workspace_clone = workspace.clone();
    let context = AgentContext {
        active_file: None, active_file_content: None,
        selection: None, open_files: Vec::new(),
        project_path: workspace_clone,
    };
    let ctx_str = context.to_prompt_context();

    // Phase 1: Planning
    println!("--- Phase 1: Planning ---");
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let stream_task = tokio::spawn(async move {
        while let Some(tok) = rx.recv().await { print!("{}", tok); }
    });

    match planner::plan_task(&llm, &prompt, &ctx_str, tx).await {
        Ok((steps, _full)) => {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            stream_task.abort();
            println!();

            let n: usize = steps.len();
            println!("Plan: {} step(s)", n);
            for (i, s) in steps.iter().enumerate() {
                println!("  [{}/{}] {} ({})", i + 1, n, s.title, s.step_type);
            }
            println!();

            if n == 0 { println!("No steps generated."); return; }

            // Phase 2: Execute each step
            let mut all_diffs: Vec<agent_ide_lib::agent::state_machine::FileDiff> = Vec::new();
            for (i, step) in steps.iter().enumerate() {
                println!("--- Step {}/{}: {} ---", i + 1, n, step.title);
                let step_ctx = format!("Task: {}\nStep: {}\nType: {}\nContext:\n{}",
                    prompt, step.title, step.step_type, ctx_str);

                let (tx2, mut rx2) = mpsc::channel::<String>(64);
                let _stream2 = tokio::spawn(async move {
                    while let Some(tok) = rx2.recv().await { print!("{}", tok); }
                });

                match executor::execute_step(&llm, &step.title, &step_ctx, tx2).await {
                    Ok(response) => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                        println!();
                        let count: usize = response.chars().count();
                        println!("  OK ({} chars)", count);

                        let diffs = executor::parse_diffs(&response);
                        if !diffs.is_empty() {
                            println!("  Diffs: {} file(s)", diffs.len());
                            all_diffs.extend(diffs);
                        }
                    }
                    Err(e) => { println!("\n  FAILED: {}", e); }
                }
                println!();
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }

            // Summary
            println!("====================");
            println!("Plan:  {} step(s)", n);
            println!("Diffs: {} file(s)", all_diffs.len());

            let mut files_written: usize = 0;
            let workspace_path = PathBuf::from(&workspace);

            for d in &all_diffs {
                println!();
                println!("--- {} ---", d.file);

                let file_rel = d.file.trim_start_matches('/').trim_start_matches('\\');
                let target_path = workspace_path.join(file_rel);

                // Display content
                for h in &d.hunks {
                    let line_count: usize = h.content.lines().count();
                    for line in h.content.lines().take(20) {
                        println!("  {}", line);
                    }
                    if line_count > 20 {
                        println!("  ... ({} more lines)", line_count - 20);
                    }

                    // Apply to filesystem if --apply
                    if apply {
                        // Extract the UPDATED content for this diff
                        let new_content = extract_updated(&h.content);

                        // Create parent dirs
                        if let Some(parent) = target_path.parent() {
                            if !parent.exists() {
                                let _ = fs::create_dir_all(parent);
                            }
                        }

                        // For new files or diffs, write the updated content
                        match fs::write(&target_path, &new_content) {
                            Ok(()) => {
                                println!("  >> Written: {}", target_path.display());
                                files_written += 1;
                            }
                            Err(e) => {
                                eprintln!("  !! Write failed for {}: {}", target_path.display(), e);
                            }
                        }
                    } else {
                        println!("  >> Preview: would write to {}", target_path.display());
                    }
                }
            }

            if apply {
                println!();
                println!("====================");
                println!("{} file(s) written to {}", files_written, workspace);
            }
            println!("====================");
        }
        Err(e) => { eprintln!("Planning failed: {}", e); }
    }
}

/// Extract the UPDATED content from a diff block.
/// For format: <<<<<<< ORIGINAL\n...\n=======\n...\n>>>>>>> UPDATED
/// Returns the updated part, or the full content if no markers found.
fn extract_updated(content: &str) -> String {
    let mut in_updated = false;
    let mut updated_lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        if line.trim().starts_with(">>>>>>>") {
            in_updated = false;
            continue;
        }
        if line.trim().starts_with("=======") {
            in_updated = true;
            continue;
        }
        if line.trim().starts_with("<<<<<<<") {
            continue;
        }
        if in_updated {
            updated_lines.push(line);
        }
    }

    if updated_lines.is_empty() {
        // No diff markers found — treat entire content as the new code
        content.to_string()
    } else {
        updated_lines.join("\n")
    }
}

fn print_help() {
    println!(r#"Agent IDE CLI - AI Coding Agent

Usage:
  agent-cli [OPTIONS] <PROMPT>

Options:
  --endpoint <URL>    LLM API endpoint (or LLM_ENDPOINT env)
  --api-key <KEY>     API key (or LLM_API_KEY env)
  --model <NAME>      Model name (or LLM_MODEL env)
  --workspace <DIR>   Project workspace directory (default: current dir)
  --apply             Write generated files to disk
  --help, -h          Show this help

Examples:
  # Preview only (no files written):
  agent-cli --endpoint https://api.deepseek.com --api-key sk-xxx --model deepseek-v4-flash "Create hello.ts"

  # Write files to disk:
  agent-cli --apply --workspace ./my-project "Create a React login component"
"#);
}
