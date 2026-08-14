use clap::Parser;
use cordis_mcp::CordisMcpServer;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "cordis-mcp",
    version,
    about = "Run CORDIS Rust as a stdio MCP server"
)]
struct Args {
    #[arg(long, default_value = ".cordis")]
    data_dir: PathBuf,
}

fn main() {
    let args = Args::parse();
    let server = CordisMcpServer::open(&args.data_dir).unwrap_or_else(|error| {
        eprintln!("failed to initialize CORDIS MCP: {error}");
        std::process::exit(1);
    });
    if let Err(error) = server.run_stdio() {
        eprintln!("CORDIS MCP stopped with an error: {error}");
        std::process::exit(1);
    }
}
