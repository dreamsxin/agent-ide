// ============================================================
// Agent IDE CLI - command-line AI coding agent
// ============================================================
// Usage:
//   agent-cli --endpoint URL --api-key KEY --model NAME "your prompt"
//   agent-cli --apply "your prompt"        (use env vars + write files)
//   or set env: LLM_ENDPOINT, LLM_API_KEY, LLM_MODEL

use agent_ide_lib::agent::diff_apply::apply_pending_diffs;
use agent_ide_lib::agent::executor;
use agent_ide_lib::agent::planner;
use agent_ide_lib::services::context::AgentContext;
use agent_ide_lib::services::llm_client::{LlmClient, LlmConfig};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::mpsc;

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
            "--endpoint" => {
                i += 1;
                if i < args.len() {
                    endpoint = args[i].clone();
                }
            }
            "--api-key" => {
                i += 1;
                if i < args.len() {
                    api_key = args[i].clone();
                }
            }
            "--model" => {
                i += 1;
                if i < args.len() {
                    model = args[i].clone();
                }
            }
            "--workspace" => {
                i += 1;
                if i < args.len() {
                    workspace = args[i].clone();
                }
            }
            "--apply" => {
                apply = true;
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other if !other.starts_with('-') => {
                if prompt.is_empty() {
                    prompt = other.to_string();
                }
            }
            _ => {
                eprintln!("Unknown flag: {}", args[i]);
                return;
            }
        }
        i += 1;
    }

    // Env fallback
    if endpoint.is_empty() {
        endpoint = std::env::var("LLM_ENDPOINT").unwrap_or_default();
    }
    if api_key.is_empty() {
        api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
    }
    if model.is_empty() {
        model = std::env::var("LLM_MODEL").unwrap_or_default();
    }

    if endpoint.is_empty() || api_key.is_empty() || model.is_empty() || prompt.is_empty() {
        return;
    }

    println!("=== Agent IDE CLI ===");
    println!("Endpoint: {}", endpoint);
    println!("Model:    {}", model);
    println!("Workspace:{}", workspace);
    println!("Prompt:   {}", prompt);
    if apply {
        println!("Mode:     Apply (files will be written)");
    } else {
        println!("Mode:     Preview only");
    }
    println!();

    let workspace_path = match PathBuf::from(&workspace).canonicalize() {
        Ok(path) if path.is_dir() => path,
        Ok(path) => {
            eprintln!("Workspace is not a directory: {}", path.display());
            return;
        }
        Err(e) => {
            eprintln!("Workspace is not accessible: {}", e);
            return;
        }
    };
    workspace = workspace_path.to_string_lossy().to_string();
    std::env::set_var("AGENT_IDE_CONFIG_DIR", workspace_path.join(".agent-ide"));
    if let Err(e) = agent_ide_lib::services::workspace::save_workspace_path(&workspace) {
        eprintln!("Failed to set workspace: {}", e);
        return;
    }

    let config = LlmConfig {
        endpoint,
        api_key,
        model,
    };
    let llm = LlmClient::new(config);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let workspace_clone = workspace.clone();
    let mut context = AgentContext {
        active_file: None,
        active_file_content: None,
        selection: None,
        open_files: Vec::new(),
        project_path: workspace_clone,
        git_diff: None,
        project_tree: None,
    };
    context.enrich_from_workspace();
    let ctx_str = context.to_prompt_context();

    // Phase 1: Planning
    println!("--- Phase 1: Planning ---");
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let stream_task = tokio::spawn(async move {
        while let Some(tok) = rx.recv().await {
            print!("{}", tok);
        }
    });

    match planner::plan_task(&llm, &prompt, &ctx_str, cancel_flag.clone(), tx).await {
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

            if n == 0 {
                println!("No steps generated.");
                return;
            }

            // Phase 2: Execute each step
            let mut all_diffs: Vec<agent_ide_lib::agent::state_machine::FileDiff> = Vec::new();
            for (i, step) in steps.iter().enumerate() {
                println!("--- Step {}/{}: {} ---", i + 1, n, step.title);
                // Build executor context with file contents
                let mut step_ctx = format!(
                    "Task: {}\nStep: {}\nType: {}\nContext:\n{}",
                    prompt, step.title, step.step_type, ctx_str
                );

                // Auto-read files mentioned in step title or context, then inject
                // their contents into the executor prompt (two-phase: collect paths, then read)
                let mut paths_to_try: Vec<std::path::PathBuf> = Vec::new();

                // Scan step title for filenames
                for word in step.title.split_whitespace() {
                    let w = word.trim_matches(|c: char| {
                        !c.is_alphanumeric()
                            && c != '.'
                            && c != '/'
                            && c != '\\'
                            && c != '-'
                            && c != '_'
                    });
                    if w.contains('.') && w.len() > 3 {
                        paths_to_try.push(workspace_path.join(w));
                        paths_to_try.push(std::path::PathBuf::from(w));
                    }
                }

                // Fallback: scan workspace for common source files
                if paths_to_try.is_empty() {
                    let exts = ["js", "ts", "jsx", "tsx", "py", "rs", "go", "java"];
                    if let Ok(entries) = std::fs::read_dir(&workspace_path) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                if exts.contains(&ext) {
                                    paths_to_try.push(p);
                                }
                            }
                        }
                    }
                }

                // Read and inject file contents
                let mut found_names: Vec<String> = Vec::new();
                for path in &paths_to_try {
                    if path.exists() && path.is_file() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if !found_names.contains(&name.to_string()) {
                                if let Ok(file_content) = std::fs::read_to_string(path) {
                                    step_ctx.push_str(&format!(
                                        "\n\n--- File: {} ---\n```\n{}\n```",
                                        name, file_content
                                    ));
                                    found_names.push(name.to_string());
                                }
                            }
                        }
                    }
                }

                if !found_names.is_empty() {
                    step_ctx
                        .push_str("\n\n(File contents above are current - base your diff on them)");
                }

                let (tx2, mut rx2) = mpsc::channel::<String>(64);
                let _stream2 = tokio::spawn(async move {
                    while let Some(tok) = rx2.recv().await {
                        print!("{}", tok);
                    }
                });

                match executor::execute_step(&llm, &step.title, &step_ctx, cancel_flag.clone(), tx2)
                    .await
                {
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
                    Err(e) => {
                        println!("\n  FAILED: {}", e);
                    }
                }
                println!();
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }

            // Summary
            println!("====================");
            println!("Plan:  {} step(s)", n);
            println!("Diffs: {} file(s)", all_diffs.len());

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

                    if !apply {
                        println!("  >> Preview: would write to {}", target_path.display());
                    }
                }
            }

            if apply {
                let result = apply_pending_diffs(&all_diffs);
                println!();
                println!("====================");
                for diff in &result.applied {
                    println!("Applied: {}", diff.file);
                }
                for failure in &result.failed {
                    println!("Failed:  {} - {}", failure.file, failure.message);
                }
                println!("{} file(s) written to {}", result.applied.len(), workspace);
            }
            println!("====================");
        }
        Err(e) => {
            eprintln!("Planning failed: {}", e);
        }
    }
}

fn print_help() {
    println!(
        r#"Agent IDE CLI - AI Coding Agent

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
"#
    );
}
