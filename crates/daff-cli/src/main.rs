use anyhow::Result;
use clap::{Parser, Subcommand};
use daff::format::{csv::Csv, TableReader};
use daff::{compare_tables, Flags, TableDiff};
use std::{fs::File, path::PathBuf};

#[derive(Parser)]
#[command(name = "daff", version, about = "Tabular diff, patch and merge")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show differences between two tables
    Diff {
        a: PathBuf,
        b: PathBuf,
        /// Column(s) identifying a row
        #[arg(long)]
        id: Vec<String>,
    },
    /// Apply a diff to a table
    Patch { table: PathBuf, patch: PathBuf },
    /// Three-way merge
    Merge { parent: PathBuf, a: PathBuf, b: PathBuf },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Diff { a, b, id } => {
            let a = Csv::default().read(&mut File::open(a)?)?;
            let b = Csv::default().read(&mut File::open(b)?)?;
            let flags = Flags { ids: id, ..Flags::default() };
            let alignment = compare_tables(&a, &b).with_flags(flags.clone()).align();
            let _diff = TableDiff::new(&alignment, flags).highlight()?;
        }
        Cmd::Patch { .. } => anyhow::bail!("patch: not implemented yet"),
        Cmd::Merge { .. } => anyhow::bail!("merge: not implemented yet"),
    }
    Ok(())
}
