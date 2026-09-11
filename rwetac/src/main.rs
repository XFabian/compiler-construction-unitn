//! Command-line interface for the rwetac compiler.
//! Run with `cargo run -- [OPTIONS] <FILE>` or after installation with `rwetac [OPTIONS] <FILE>`.
//!
//! # Examples
//!
//! ```text
//! rwetac program.eta                    # Full compilation to WASM (default)
//! rwetac --parse program.eta            # Stop after parsing, print AST
//! rwetac --validate program.eta         # Stop after typechecking
//! rwetac --wtac program.eta             # Stop after IR generation, print WTAC
//! rwetac --dump --opt program.eta       # Full compilation with optimization, write debug files
//! rwetac --report-passes                # List the optimization passes available
//! rwetac --passes cf,dce program.eta    # Run just these two passes, in this order
//! rwetac --passes 'fixpoint(cp,cf)' program.eta   # Repeat until nothing changes
//! rwetac --opt --count program.eta      # Report WTAC instruction counts before and after
//! ```
//!
//! The `--dump` flag writes intermediate representations to a `<file>_debug/` directory,
//! including the WTAC IR and CFG graphs as `.dot` files (viewable with Graphviz).
//!
//! Tracing verbosity is controlled via the `RUST_LOG` environment variable.
use clap::Parser;
use std::path;

use rwetac::compile;
use rwetac::setting::{Pass, Pipeline, Stage};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
#[derive(Parser, Debug)]
#[command(name = "rwtac", version = "0.1", about = "A Rust Compiler for Eta")]
struct Cli {
    /// Parse the source file into an AST
    #[arg(long, group = "stage")]
    parse: bool,

    /// Perform semantic validation
    #[arg(long, group = "stage")]
    validate: bool,

    /// Lower to the WTac intermediate representation
    #[arg(long, group = "stage")]
    wtac: bool,

    /// Run the Code Generation to WASM
    #[arg(long, group = "stage")]
    codegen: bool,
    /// The source file to compile. Not needed with --report-passes.
    #[arg(value_name = "FILE")]
    file: Option<String>,

    /// Dump Debug files like IR and CFG (if optimization is turned on). Written to [file]_debug/
    #[arg(long)]
    dump: bool,

    /// Run every optimization to a fixed point. Shorthand for the default pipeline.
    #[arg(long)]
    opt: bool,

    /// Run this optimization pipeline, in order. Overrides --opt.
    ///
    /// A comma-separated list of pass names, where a group written
    /// `fixpoint(a,b)` repeats until it stops changing anything.
    /// Use --report-passes to list the available names.
    #[arg(long, value_name = "LIST")]
    passes: Option<String>,

    /// Print the optimization passes this compiler supports, then exit
    #[arg(long)]
    report_passes: bool,

    /// Print WTAC instruction counts before and after optimization
    #[arg(long)]
    count: bool,
}

impl Cli {
    fn stage(&self) -> Stage {
        if self.parse {
            Stage::Parse
        } else if self.validate {
            Stage::Validate
        } else if self.wtac {
            Stage::Wtac
        } else {
            // --codegen, and also the default: run the whole pipeline.
            Stage::Codegen
        }
    }

    /// `--passes` wins over `--opt`; with neither, nothing is optimized.
    fn pipeline(&self) -> anyhow::Result<Option<Pipeline>> {
        match &self.passes {
            Some(spec) => Pipeline::parse(spec)
                .map(Some)
                .map_err(|e| anyhow::anyhow!("Invalid --passes: {e}")),
            None if self.opt => Ok(Some(Pipeline::default_full())),
            None => Ok(None),
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    if args.report_passes {
        for pass in Pass::ALL {
            println!("{:<6} {}", pass.name(), pass.describe());
        }
        return Ok(());
    }

    let file = args
        .file
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No source file given"))?;
    let path = path::Path::new(file);
    let pipeline = args.pipeline()?;
    // Quiet by default; RUST_LOG opts into the phase tracing.
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("warn"))
        .unwrap();

    // Now, build the subscriber with the filter we've created.
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(true)
        .without_time()
        .with_span_events(FmtSpan::ENTER)
        .init();

    compile::compile(args.stage(), args.dump, pipeline, args.count, path)?;

    Ok(())
}
