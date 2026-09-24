//! Writes nit's public contract into the directory named by the first
//! argument: `openapi.json`, the document the routes' utoipa annotations
//! produce, and `cli.json`, the syntax of every `nit` command.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::CommandFactory;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let [_, dir] = args.as_slice() else {
        bail!("usage: nit-contract <dir>");
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("openapi.json"),
        nit::api::openapi().to_pretty_json()?,
    )?;
    // Building assigns each positional its index.
    let mut cli = nit::command::Args::command();
    cli.build();
    std::fs::write(
        dir.join("cli.json"),
        serde_json::to_string_pretty(&command(&cli))?,
    )?;
    Ok(())
}

/// The syntax a caller of `cmd` relies on: its arguments and subcommands,
/// with no help text.
///
/// An argument is named by its flag or its position, never by the Rust
/// field behind it, which a caller never sees.
fn command(cmd: &clap::Command) -> serde_json::Value {
    let args: Vec<_> = cmd
        .get_arguments()
        .filter(|arg| {
            !matches!(
                arg.get_action(),
                clap::ArgAction::Help
                    | clap::ArgAction::HelpShort
                    | clap::ArgAction::HelpLong
                    | clap::ArgAction::Version
            )
        })
        .map(|arg| {
            serde_json::json!({
                "long": arg.get_long(),
                "short": arg.get_short(),
                "index": arg.get_index(),
                "required": arg.is_required_set(),
                "takes_value": arg.get_action().takes_values(),
                "multiple": matches!(arg.get_action(), clap::ArgAction::Append | clap::ArgAction::Count),
                "possible_values": arg
                    .get_possible_values()
                    .iter()
                    .map(clap::builder::PossibleValue::get_name)
                    .collect::<Vec<_>>(),
                "default_values": arg
                    .get_default_values()
                    .iter()
                    .map(|v| v.to_string_lossy())
                    .collect::<Vec<_>>(),
                "global": arg.is_global_set(),
                "hidden": arg.is_hide_set(),
            })
        })
        .collect();
    let subcommands: serde_json::Map<_, _> = cmd
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help")
        .map(|sub| (sub.get_name().to_string(), command(sub)))
        .collect();
    serde_json::json!({ "args": args, "subcommands": subcommands })
}
