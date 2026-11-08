//! `simplefp`: every subcommand except `mcp` comes from the tool registry (`simplefp::tools`), so the CLI, MCP and
//! future front-ends share one definition, one validator and one result per tool.

use clap::{Arg, ArgAction, Command};
use simplefp::tools::{self, TOOLS};
use std::process::ExitCode;

mod mcp;

fn cli() -> Command {
    let mut c = Command::new("simplefp")
        .version(simplefp::VERSION)
        .about("Fiber photometry analysis for TDT recordings.")
        .subcommand_required(true)
        .arg(Arg::new("json").long("json").global(true).action(ArgAction::SetTrue).help("Print the full JSON result instead of the summary"))
        .subcommand(Command::new("mcp").about("Serve SimpleFP's tools to AI agents over the Model Context Protocol (stdio)"));
    for t in TOOLS {
        c = c.subcommand((t.command)());
    }
    c
}

fn main() -> ExitCode {
    let m = cli().get_matches();
    let result = match m.subcommand() {
        Some(("mcp", _)) => mcp::serve().map(|()| None),
        Some((name, sub)) => (tools::find(name).expect("subcommands come from TOOLS").call_cli)(sub).map(Some),
        None => unreachable!("subcommand_required"),
    };
    match result {
        Ok(Some(v)) if m.get_flag("json") => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
        Ok(Some(v)) => print!("{}", v["summary"].as_str().unwrap_or_default()),
        Ok(None) => {}
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
