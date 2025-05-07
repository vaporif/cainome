use clap::{Parser, Subcommand};
use xshell::{Shell, cmd};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Deny {
        #[clap(last = true)]
        args: Vec<String>,
    },
    Doc,
    UnusedDeps,
}

fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    let sh = Shell::new()?;
    let args = Args::parse();

    match args.command {
        Commands::Deny { args } => {
            println!("cargo deny");
            cmd!(sh, "cargo install --version 0.18.2 cargo-deny").run()?;
            cmd!(sh, "cargo deny check {args...}").run()?;
        }
        Commands::Doc => {
            println!("cargo doc");
            cmd!(sh, "cargo doc --workspace --no-deps --all-features").run()?;

            if std::option_env!("CI").is_none() {
                #[cfg(target_os = "macos")]
                cmd!(sh, "open target/doc/cainome/index.html").run()?;

                #[cfg(target_os = "linux")]
                cmd!(sh, "xdg-open target/doc/cainome/index.html").run()?;
            }
        }
        Commands::UnusedDeps => {
            println!("unused deps");
            cmd!(sh, "cargo install --version 0.8.0 cargo-machete").run()?;
            cmd!(sh, "cargo-machete").run()?;
        }
    }

    Ok(())
}
