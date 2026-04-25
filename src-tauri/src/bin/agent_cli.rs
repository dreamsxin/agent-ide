// ============================================================
// Agent IDE CLI — command-line AI coding agent
// ============================================================
// Usage:
//   agent-cli --endpoint URL --api-key KEY --model NAME "your prompt"
//   or set env: LLM_ENDPOINT, LLM_API_KEY, LLM_MODEL

use agent_ide_lib::agent::planner;
use agent_ide_lib::agent::executor;
use agent_ide_lib::services::llm_client::{LlmClient, LlmConfig};
use agent_ide_lib::services::context::AgentContext;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut endpoint = String::new();
    let mut api_key = String::new();
    let mut model = String::new();
    let mut prompt = String::new();
    let workspace = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--endpoint" => { i += 1; if i < args.len() { endpoint = args[i].clone(); } }
            "--api-key" => { i += 1; if i < args.len() { api_key = args[i].clone(); } }
            "--model" => { i += 1; if i < args.len() { model = args[i].clone(); } }
            "--workspace" => { i += 1; if i < args.len() { /*workspace = args[i].clone();*/ let _ = &args[i]; } }
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
    println!("Prompt:   {}", prompt);
    println!();

    let config = LlmConfig { endpoint, api_key, model };
    let llm = LlmClient::new(config);

    let context = AgentContext {
        active_file: None, active_file_content: None,
        selection: None, open_files: Vec::new(),
        project_path: workspace,
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
            // Allow stream to flush
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            stream_task.abort();
            println!();

            let n = steps.len();
            println!("Plan: {} step(s)", n);
            for (i, s) in steps.iter().enumerate() {
                println!("  [{}/{}] {} ({})", i + 1, n, s.title, s.step_type);
            }
            println!();

            if n == 0 { println!("No steps generated."); return; }

            // Phase 2: Execute each step
            let mut all_diffs = Vec::new();
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
                        let count = response.chars().count();
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
            for d in &all_diffs {
                println!();
                println!("--- {} ---", d.file);
                for h in &d.hunks {
                    for line in h.content.lines().take(20) {
                        println!("  {}", line);
                    }
                    if h.content.lines().count() > 20 {
                        println!("  ... ({} more lines)", h.content.lines().count() - 20);
                    }
                }
            }
            println!("====================");
        }
        Err(e) => { eprintln!("Planning failed: {}", e); }
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
  --workspace <DIR>   Project workspace directory
  --help, -h          Show this help

Examples:
  agent-cli --endpoint https://api.deepseek.com --api-key sk-xxx --model deepseek-v4-flash "Create hello.ts"
"#);
}
