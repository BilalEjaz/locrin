mod run;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "locrin",
    about = "Deterministic quality gate for code written by people and agents",
    long_about = "Deterministic quality gate for code written by people and agents.\n\n\
                  The index lives outside the repository, in the platform cache directory. Set \
                  LOCRIN_CACHE_DIR to put it somewhere else: give a CI job or a sandbox its own \
                  cache so runs do not share incremental state."
)]
struct Cli {
    /// Repository root (defaults to the current directory)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check files (all by default) and print a verdict
    Check {
        paths: Vec<PathBuf>,
        /// Only files whose content changed since the last index, plus the
        /// graph findings their edges reach.
        #[arg(long)]
        changed: bool,
        /// Compact JSON for agents (capped at ten findings)
        #[arg(long)]
        json: bool,
    },
    /// Index the repository and warm the findings cache without printing a verdict
    Scan,
    /// Manage the baseline of accepted findings
    Baseline {
        #[command(subcommand)]
        cmd: BaselineCmd,
    },
}

#[derive(Subcommand)]
enum BaselineCmd {
    /// Snapshot every current finding into the baseline
    Create,
    /// Accept one finding by id with a reason
    Accept {
        id: String,
        #[arg(long)]
        reason: String,
    },
}

fn real_main() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    let root = match cli.root {
        Some(r) => r,
        None => std::env::current_dir()?,
    };
    match cli.cmd {
        Cmd::Check { paths, changed, json } => {
            let opts = run::Options { root, paths, changed_only: changed, json };
            let verdict = run::check(&opts)?;
            if opts.json {
                println!("{}", locrin_reporters::agent::render(&verdict));
            } else {
                print!("{}", locrin_reporters::terminal::render(&verdict));
            }
            Ok(verdict.exit_code())
        }
        Cmd::Scan => {
            let (files, changed) = run::scan(&root)?;
            println!("indexed {files} file(s), {changed} changed");
            Ok(0)
        }
        Cmd::Baseline { cmd: BaselineCmd::Create } => {
            let n = run::baseline_create(&root)?;
            println!("baseline written with {n} finding(s)");
            Ok(0)
        }
        Cmd::Baseline { cmd: BaselineCmd::Accept { id, reason } } => {
            if run::baseline_accept(&root, &id, &reason)? {
                println!("accepted {id}");
                Ok(0)
            } else {
                eprintln!("error: no current finding with id {id}");
                Ok(2)
            }
        }
    }
}

fn main() -> ExitCode {
    // The default hook prints its own multi-line panic report before the
    // unwind reaches us, so a caught panic would speak twice. Silence it and
    // let the arm below be the single line an operator reads.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(real_main);
    match result {
        Ok(Ok(code)) => ExitCode::from(code as u8),
        Ok(Err(e)) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
        Err(_) => {
            eprintln!("error: internal engine failure");
            ExitCode::from(2)
        }
    }
}
