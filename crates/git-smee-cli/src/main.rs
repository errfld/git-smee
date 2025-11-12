use clap::{Parser, command};

#[derive(clap::Parser)]
#[command(name = "git-smee")]
#[command(about = "🏴‍☠️ Smee - the right hand of (Git) hooks", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Install,
    Run { hook: String },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Install => {
            println!("Installing hooks...");
            // Installation logic goes here
        }
        Command::Run { hook } => {
            println!("Running hook: {hook}");
            // Hook execution logic goes here
        }
    }
}
