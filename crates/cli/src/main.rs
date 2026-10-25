use clap::Parser;

fn main() {
    let cli = charted_cli::Cli::parse();
    match cli.command {
        Some(cmd) => std::process::exit(charted_cli::run(cmd)),
        None => {
            eprintln!("charted-headless has no GUI; run `charted-headless --help` for commands");
            std::process::exit(1);
        }
    }
}
