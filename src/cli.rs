use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum EmitType {
    Bytecode,
    Object,
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
        #[arg(short = 'b', long = "bytecode")]
        emit_bytecode: bool,
        /// emit relocatable object file (.o) without linking
        #[arg(short = 'c', long = "obj")]
        emit_object: bool,
        #[arg(short, long)]
        run: bool,
        /// print loaded files and bytecode disassembly to stderr
        #[arg(short = 'd', long = "debug")]
        debug: bool,
        /// explicit linker binary (overrides VOID_LINKER env var)
        #[arg(long = "linker")]
        linker: Option<PathBuf>,
        /// strip debug symbols from the output binary
        #[arg(short = 's', long = "strip")]
        strip: bool,
    },
    /// build and run project (reads void.toml)
    Run {
        /// explicit linker binary
        #[arg(long = "linker")]
        linker: Option<PathBuf>,
        /// strip debug symbols from the output binary
        #[arg(short = 's', long = "strip")]
        strip: bool,
    },
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
        #[arg(short = 'b', long = "bytecode")]
        emit_bytecode: bool,
    },
    /// start language server (stdio mode)
    Lsp {
        #[arg(long, default_value_t = true)]
        stdio: bool,
    },
}
