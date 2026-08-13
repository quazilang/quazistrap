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
pub enum TargetTriple {
    #[value(name = "x86_64-linux")]
    X86_64Linux,
    #[value(name = "x86_64-windows")]
    X86_64Windows,
}

pub type HeaderTarget = TargetTriple;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DependencyType {
    Git,
    Archive,
    Source,
    Qzi,
}

/// Quazi compiler and package toolchain
#[derive(Parser, Debug)]
#[command(name = "qz", version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Subcommand, Debug)]
pub enum Command {
    /// Build source, QZI, or the current project
    Build {
        /// Quazi source/QZI files and optional native object inputs
        files: Vec<PathBuf>,
        /// output file name
        #[arg(short, long)]
        output: Option<String>,
        /// emit portable QZI bytecode instead of native output
        #[arg(short = 'i', long = "bytecode")]
        emit_bytecode: bool,
        /// emit relocatable object file (.o) without linking
        #[arg(short = 'c', long = "obj")]
        emit_object: bool,
        /// run the binary after a successful build
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
        /// ignore and do not update the project incremental cache
        #[arg(long)]
        no_incremental: bool,
        /// build named binary artifact
        #[arg(long, conflicts_with = "lib")]
        bin: Option<String>,
        /// build library artifact
        #[arg(long, conflicts_with = "bin")]
        lib: bool,
        /// native compilation target
        #[arg(long, value_enum)]
        target: Option<TargetTriple>,
        /// suppress successful build output and warnings; errors still print
        #[arg(short = 'q', long = "silent")]
        silent: bool,
        /// disable ANSI colors even when stderr is a terminal
        #[arg(long)]
        no_color: bool,
        /// use ASCII-only build output
        #[arg(long)]
        no_unicode: bool,
        /// hide build stages and print only `built <name>` on success
        #[arg(long)]
        no_progress: bool,
    },
    /// Build and run source, QZI, or the current project
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
        /// ignore and do not update the project incremental cache
        #[arg(long)]
        no_incremental: bool,
        #[arg(long, conflicts_with = "lib")]
        bin: Option<String>,
        #[arg(long, conflicts_with = "bin")]
        lib: bool,
        #[arg(long, value_enum)]
        target: Option<TargetTriple>,
        /// suppress successful build output and warnings; errors still print
        #[arg(short = 'q', long = "silent")]
        silent: bool,
        /// disable ANSI colors even when stderr is a terminal
        #[arg(long)]
        no_color: bool,
        /// use ASCII-only build output
        #[arg(long)]
        no_unicode: bool,
        /// hide build stages and print only `built <name>` on success
        #[arg(long)]
        no_progress: bool,
    },
    /// Type-check the current project without code generation
    Check {
        #[arg(long, conflicts_with = "lib")]
        bin: Option<String>,
        #[arg(long, conflicts_with = "bin")]
        lib: bool,
        #[arg(long, value_enum)]
        target: Option<TargetTriple>,
    },
    /// Download, verify, and lock project dependencies
    Fetch,
    /// Show resolved dependencies and local cache paths
    Deps,
    /// Add a dependency and refresh quazi.lock
    Add {
        /// local path or internet URL
        dependency: String,
        /// internet dependency format
        #[arg(long = "type", value_enum)]
        kind: Option<DependencyType>,
        /// local import name; package identity remains validated from metadata
        #[arg(long)]
        alias: Option<String>,
        /// Git tag, commit hash, or `latest`; QZI/package version otherwise
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        checksum: Option<String>,
    },
    /// Remove a dependency and refresh quazi.lock
    Remove { name: String },
    /// Generate a C header for exported functions and compatible types
    Header {
        /// source files, or the current project when omitted
        files: Vec<PathBuf>,
        /// generated header path
        #[arg(short, long, default_value = "quazi.h")]
        output: PathBuf,
        /// C data model used for cfg and platform aliases
        #[arg(long, value_enum, default_value = "x86_64-linux")]
        target: TargetTriple,
    },
    /// Create a new project
    New {
        name: String,
        /// create a library project instead of a binary
        #[arg(short = 'l', long = "lib")]
        lib: bool,
    },
    /// Initialize a project in the current directory
    Init {
        /// create a library project instead of a binary
        #[arg(short = 'l', long = "lib")]
        lib: bool,
    },
    /// Format project source files
    Fmt,
    /// Remove project build artifacts
    Clean,
    /// Run the compiler's internal debug program
    Debug {
        #[arg(short = 'i', long = "bytecode")]
        emit_bytecode: bool,
    },
    /// Start the language server
    Lsp {
        #[arg(long, default_value_t = true)]
        stdio: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::{Args, Command, DependencyType};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn project_gitignore_keeps_lockfile_tracked() {
        assert!(super::super::PROJECT_GITIGNORE.contains("/build/"));
        assert!(super::super::PROJECT_GITIGNORE.contains("*.qzi"));
        assert!(!super::super::PROJECT_GITIGNORE.contains("quazi.lock"));
        assert!(!super::super::PROJECT_GITIGNORE.contains("*.lock"));
    }

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
    fn build_accepts_output_control_flags() {
        let args = Args::try_parse_from([
            "qz",
            "build",
            "--silent",
            "--no-color",
            "--no-unicode",
            "--no-progress",
        ])
        .expect("output controls should parse");

        assert!(matches!(
            args.command,
            Command::Build {
                silent: true,
                no_color: true,
                no_unicode: true,
                no_progress: true,
                ..
            }
        ));
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
                assert_eq!(target, super::TargetTriple::X86_64Windows);
            }
            command => panic!("expected header command, got {command:?}"),
        }
    }

    #[test]
    fn add_accepts_local_and_remote_dependencies() {
        let local =
            Args::try_parse_from(["qz", "add", "../math"]).expect("local dependency should parse");
        assert!(
            matches!(local.command, Command::Add { dependency, .. } if dependency == "../math")
        );

        let remote =
            Args::try_parse_from(["qz", "add", "https://example.test/web.qzi", "--type", "qzi"])
                .expect("remote dependency should parse");
        assert!(
            matches!(remote.command, Command::Add { dependency, .. } if dependency == "https://example.test/web.qzi")
        );

        let inferred_local = Args::try_parse_from(["qz", "add", "../math"])
            .expect("path-only dependency should parse");
        assert!(matches!(
            inferred_local.command,
            Command::Add { dependency, .. } if dependency == "../math"
        ));

        let inferred_remote = Args::try_parse_from([
            "qz",
            "add",
            "https://example.test/qz-math.git",
            "--type",
            "git",
        ])
        .expect("URL-only dependency should parse");
        assert!(matches!(
            inferred_remote.command,
            Command::Add { dependency, kind: Some(DependencyType::Git), .. }
                if dependency == "https://example.test/qz-math.git"
        ));

        let aliased = Args::try_parse_from(["qz", "add", "../math", "--alias", "numbers"])
            .expect("aliased dependency should parse");
        assert!(matches!(
            aliased.command,
            Command::Add { alias: Some(alias), .. } if alias == "numbers"
        ));

        assert!(Args::try_parse_from(["qz", "add", "math", "--path", "../math"]).is_err());
    }
}
