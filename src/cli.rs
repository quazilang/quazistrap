// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum EmitType {
    Bytecode,
    Object,
    Binary,
}

/// quazilang compiler
#[derive(Parser, Debug)]
#[command(name = "qz", version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Subcommand, Debug)]
pub enum Command {
    /// build files or project (if no files given, reads quazi.toml)
    Build {
        files: Vec<PathBuf>,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(short = 'i', long = "bytecode")]
        emit_bytecode: bool,
        /// emit relocatable object file (.o) without linking
        #[arg(short = 'c', long = "obj")]
        emit_object: bool,
        #[arg(short, long)]
        run: bool,
        /// print loaded files and bytecode disassembly to stderr
        #[arg(short = 'd', long = "debug")]
        debug: bool,
        /// explicit linker binary (overrides QUAZI_LINKER env var)
        #[arg(long = "linker")]
        linker: Option<PathBuf>,
        /// strip debug symbols from the output binary
        #[arg(short = 's', long = "strip")]
        strip: bool,
    },
    /// build and run project (reads quazi.toml)
    Run {
        /// explicit linker binary
        #[arg(long = "linker")]
        linker: Option<PathBuf>,
        /// strip debug symbols from the output binary
        #[arg(short = 's', long = "strip")]
        strip: bool,
    },
    /// check project without compiling (reads quazi.toml)
    Check,
    /// create new project
    New {
        name: String,
        /// create a library project instead of a binary
        #[arg(short = 'l', long = "lib")]
        lib: bool,
    },
    /// initialize a project in the current directory
    Init {
        /// create a library project instead of a binary
        #[arg(short = 'l', long = "lib")]
        lib: bool,
    },
    /// format source files
    Fmt,
    /// clean build artifacts
    Clean,
    /// debug (use preset code and nothing more)
    Debug {
        #[arg(short = 'i', long = "bytecode")]
        emit_bytecode: bool,
    },
    /// start language server (stdio mode)
    Lsp {
        #[arg(long, default_value_t = true)]
        stdio: bool,
    },
}
