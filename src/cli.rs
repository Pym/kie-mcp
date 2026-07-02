use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use serde_json::Value;
use tracing_subscriber::EnvFilter;

use crate::{
    config::Config,
    kie::{
        KieClient, catalog,
        jobs::{GenerationKind, GenerationRequest, model_kind},
    },
    mcp,
};

#[derive(Debug, Parser)]
#[command(name = "kie-mcp", about = "Kie.ai image/video MCP stdio server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve,
    Debug {
        #[command(subcommand)]
        command: DebugCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DebugCommand {
    Credits,
    Upload(UploadArgs),
    Models(ModelsArgs),
    Create(CreateArgs),
    Wait(WaitArgs),
}

#[derive(Debug, Args)]
struct UploadArgs {
    path: PathBuf,
}

#[derive(Debug, Args)]
struct ModelsArgs {
    #[arg(long)]
    media_type: Option<String>,
    #[arg(long)]
    query: Option<String>,
}

#[derive(Debug, Args)]
struct CreateArgs {
    #[arg(long)]
    model: String,
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    media_type: Option<String>,
}

#[derive(Debug, Args)]
struct WaitArgs {
    task_id: String,
    #[arg(long)]
    download: bool,
    #[arg(long, default_value = "image")]
    media_type: String,
}

pub async fn run() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => mcp::serve_stdio().await,
        Command::Debug { command } => run_debug(command).await,
    }
}

async fn run_debug(command: DebugCommand) -> Result<()> {
    let client = KieClient::new(Config::from_env()?);
    match command {
        DebugCommand::Credits => {
            let result = client.credits().await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        DebugCommand::Upload(args) => {
            let result = client.upload_file(&args.path).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "path": result.path,
                    "url": result.url,
                }))?
            );
        }
        DebugCommand::Models(args) => {
            let kind = args
                .media_type
                .as_deref()
                .map(parse_generation_kind)
                .transpose()?;
            let models = crate::kie::catalog::models_for(kind, args.query.as_deref());
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "models": models }))?
            );
        }
        DebugCommand::Create(args) => {
            let input = tokio::fs::read_to_string(args.input).await?;
            let value: Value = serde_json::from_str(&input)?;
            let prompt = value
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let request = GenerationRequest {
                model: args.model,
                prompt,
                input_urls: Vec::new(),
                local_input_paths: Vec::new(),
                input: value,
                aspect_ratio: None,
                resolution: None,
                output_format: None,
                output_name: None,
            };
            let kind = match args.media_type.as_deref() {
                Some(media_type) => parse_generation_kind(media_type)?,
                None => catalog::resolve_model_any_kind(&request.model)
                    .map(|spec| spec.kind)
                    .or_else(|| model_kind(&request.model))
                    .ok_or_else(|| {
                        anyhow::anyhow!("unsupported or ambiguous model: {}", request.model)
                    })?,
            };
            let task_id = client.create_task(&request, kind).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "task_id": task_id }))?
            );
        }
        DebugCommand::Wait(args) => {
            let record = client.wait_for_success(&args.task_id).await?;
            if args.download {
                let kind = parse_generation_kind(&args.media_type)?;
                let result = client.download_completed(record, kind, None).await?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&KieClient::result_to_json(&result))?
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&record)?);
            }
        }
    }
    Ok(())
}

fn parse_generation_kind(value: &str) -> Result<GenerationKind> {
    match value {
        "image" => Ok(GenerationKind::Image),
        "video" => Ok(GenerationKind::Video),
        _ => anyhow::bail!("unsupported media type: {value}"),
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
