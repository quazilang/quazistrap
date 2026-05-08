use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum EmitType {
    Bytecode,
    Assembly,
    Binary,
}

/// void language compiler
#[derive(Parser, Debug)]
#[command(name = "void", version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Subcommand, Debug)]
pub enum Command {
    /// build files or project (if no files given, reads void.toml)
    Build {
        files: Vec<PathBuf>,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(short = 's', long = "asm")]
        emit_asm: bool,
        #[arg(short = 'b', long = "bytecode")]
        emit_bytecode: bool,
        #[arg(short, long)]
        run: bool,
    },
    /// build and run project (reads void.toml)
    Run,
    /// check project without compiling (reads void.toml)
    Check,
    /// create new project
    New { name: String },
    /// format source files
    Fmt,
    /// clean build artifacts
    Clean,
    /// debug (use preset code and nothing more)
    Debug {
        #[arg(short = 's', long = "asm")]
        emit_asm: bool,
        #[arg(short = 'b', long = "bytecode")]
        emit_bytecode: bool,
    },
}
