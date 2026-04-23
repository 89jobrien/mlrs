use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mlrs_core::Rlm;
use mlrs_providers::{AnthropicProvider, OpenAiProvider};
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(name = "mlrs", about = "Recursive Language Model inference")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Provider: openai or anthropic (overrides RSLM_PROVIDER)
    #[arg(long, global = true)]
    provider: Option<String>,

    /// Model ID (overrides RSLM_MODEL)
    #[arg(long, global = true)]
    model: Option<String>,

    /// Maximum recursion depth
    #[arg(long, global = true, default_value = "5")]
    max_depth: usize,

    /// Stream recursive cell trace to stderr
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a query against a context
    Query {
        query: String,
        /// Context string
        #[arg(long)]
        context: Option<String>,
        /// Path to context file
        #[arg(long)]
        context_file: Option<PathBuf>,
    },
    /// Interactive REPL mode
    Interactive,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mlrs=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let provider_name = cli
        .provider
        .clone()
        .or_else(|| std::env::var("RSLM_PROVIDER").ok())
        .unwrap_or_else(|| "openai".to_string());

    let model = cli
        .model
        .clone()
        .or_else(|| std::env::var("RSLM_MODEL").ok());

    let provider: Arc<dyn mlrs_core::LlmProvider> = match provider_name.as_str() {
        "anthropic" => {
            let p = if let Some(m) = model {
                AnthropicProvider::new(m)?
            } else {
                AnthropicProvider::from_env()?
            };
            Arc::new(p)
        }
        _ => {
            let p = if let Some(m) = model {
                OpenAiProvider::new(m)
            } else {
                OpenAiProvider::from_env()
            };
            Arc::new(p)
        }
    };

    let rlm = Rlm::new(Arc::clone(&provider))
        .with_max_depth(cli.max_depth)
        .with_verbose(cli.verbose);

    match cli.command {
        Commands::Query {
            query,
            context,
            context_file,
        } => {
            let ctx = resolve_context(context, context_file)?;
            let answer = rlm.run(&query, &ctx).await?;
            println!("{answer}");
        }
        Commands::Interactive => {
            run_interactive(rlm).await?;
        }
    }

    Ok(())
}

fn resolve_context(context: Option<String>, context_file: Option<PathBuf>) -> Result<String> {
    if let Some(c) = context {
        return Ok(c);
    }
    if let Some(p) = context_file {
        return std::fs::read_to_string(&p)
            .with_context(|| format!("read context file: {}", p.display()));
    }
    Ok(String::new())
}

async fn run_interactive(rlm: Rlm) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let stdin = io::stdin();
    let stdout = io::stdout();

    print!("query> ");
    stdout.lock().flush()?;
    let mut query = String::new();
    stdin.lock().read_line(&mut query)?;
    let query = query.trim().to_string();

    print!("context (leave blank to skip, or enter a file path)> ");
    stdout.lock().flush()?;
    let mut ctx_input = String::new();
    stdin.lock().read_line(&mut ctx_input)?;
    let ctx_input = ctx_input.trim();

    let context = if ctx_input.is_empty() {
        String::new()
    } else if std::path::Path::new(ctx_input).exists() {
        std::fs::read_to_string(ctx_input)
            .with_context(|| format!("read context file: {ctx_input}"))?
    } else {
        ctx_input.to_string()
    };

    let answer = rlm.run(&query, &context).await?;
    println!("\nAnswer: {answer}");
    Ok(())
}
