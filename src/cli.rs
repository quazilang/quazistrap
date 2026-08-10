// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum EmitType {
    Bytecode,
    Object,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HeaderTarget {
    #[value(name = "x86_64-linux")]
    X86_64Linux,
    #[value(name = "x86_64-windows")]
    X86_64Windows,
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
        /// Quazi source/QZI files and optional native object inputs
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
        /// external linker path, or `builtin` for the experimental in-process linker
        #[arg(long = "linker")]
        linker: Option<PathBuf>,
        /// strip debug symbols from the output binary
        #[arg(short = 's', long = "strip")]
        strip: bool,
        /// add a native library search directory
        #[arg(short = 'L', value_name = "DIR")]
        library_paths: Vec<PathBuf>,
        /// link a native library (uses the platform linker's -l convention)
        #[arg(short = 'l', value_name = "NAME")]
        libraries: Vec<String>,
        /// emit a native static library (.a)
        #[arg(long = "static-lib", conflicts_with = "shared_lib")]
        static_lib: bool,
        /// emit a native shared library (.so on Linux)
        #[arg(long = "shared-lib", conflicts_with = "static_lib")]
        shared_lib: bool,
    },
    /// build and run files or project (if no files given, reads quazi.toml)
    Run {
        /// Quazi source/QZI files and optional native object inputs
        files: Vec<PathBuf>,
        /// external linker path, or `builtin` for the experimental in-process linker
        #[arg(long = "linker")]
        linker: Option<PathBuf>,
        /// strip debug symbols from the output binary
        #[arg(short = 's', long = "strip")]
        strip: bool,
        /// add a native library search directory
        #[arg(short = 'L', value_name = "DIR")]
        library_paths: Vec<PathBuf>,
        /// link a native library (uses the platform linker's -l convention)
        #[arg(short = 'l', value_name = "NAME")]
        libraries: Vec<String>,
    },
    /// check project without compiling (reads quazi.toml)
    Check,
    /// generate a C header for exported functions and C-compatible types
    Header {
        /// source files, or the current project when omitted
        files: Vec<PathBuf>,
        /// generated header path
        #[arg(short, long, default_value = "quazi.h")]
        output: PathBuf,
        /// C data model used for cfg and platform aliases
        #[arg(long, value_enum, default_value = "x86_64-linux")]
        target: HeaderTarget,
    },
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

#[cfg(test)]
mod tests {
    use super::{Args, Command};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn run_without_files_selects_project_mode() {
        let args = Args::try_parse_from(["qz", "run"]).expect("run should parse");
        match args.command {
            Command::Run { files, .. } => assert!(files.is_empty()),
            command => panic!("expected run command, got {command:?}"),
        }
    }

    #[test]
    fn run_accepts_source_and_native_inputs() {
        let args = Args::try_parse_from([
            "qz",
            "run",
            "src/main.qz",
            "native/helper.o",
            "native/helper.c",
            "-L",
            "native/lib",
            "-l",
            "helper",
        ])
        .expect("run inputs should parse");

        match args.command {
            Command::Run {
                files,
                library_paths,
                libraries,
                ..
            } => {
                assert_eq!(
                    files,
                    [
                        PathBuf::from("src/main.qz"),
                        PathBuf::from("native/helper.o"),
                        PathBuf::from("native/helper.c")
                    ]
                );
                assert_eq!(library_paths, [PathBuf::from("native/lib")]);
                assert_eq!(libraries, ["helper"]);
            }
            command => panic!("expected run command, got {command:?}"),
        }
    }

    #[test]
    fn header_accepts_files_output_and_target() {
        let args = Args::try_parse_from([
            "qz",
            "header",
            "src/lib.qz",
            "-o",
            "include/api.h",
            "--target",
            "x86_64-windows",
        ])
        .expect("header command should parse");
        match args.command {
            Command::Header {
                files,
                output,
                target,
            } => {
                assert_eq!(files, [PathBuf::from("src/lib.qz")]);
                assert_eq!(output, PathBuf::from("include/api.h"));
                assert_eq!(target, super::HeaderTarget::X86_64Windows);
            }
            command => panic!("expected header command, got {command:?}"),
        }
    }
}
