//! The main compilation driver.
//!
//! The [`compile`] function chains all compilation phases together in sequence:
//! lexing → parsing → resolution → typechecking → WTAC generation → optimization → WASM emission.
//!
//! Compilation can be stopped at any intermediate [`Stage`],
//! which prints the current representation and exits. The `dump` flag writes
//! intermediate files (WTAC IR, CFG graphs) to a `<file>_debug/` directory
//! for inspection.

use anyhow::Ok;
use std::fs;
use std::io::BufWriter;
use std::path::Path;
use tracing::{Level, info, span};

use crate::emitter;
use crate::lexer;
use crate::opt::optimizer;
use crate::parser;
use crate::resolver::Resolver;
use crate::setting::{Pipeline, Stage};
use crate::source_map::SourceMap;
use crate::typecheck::Typechecker;
use crate::wtac;
use crate::wtac::wtac_gen::WtacGen;
use anyhow::Context;

/// Compiles a single `.eta` source file.
///
/// Runs each phase in order, stopping early if `stage` is not [`Stage::Codegen`].
/// When `dump` is `true`, intermediate representations are written to disk.
/// When `pipeline` is `Some`, the WTAC IR is optimized with it before emission.
/// When `count` is `true`, WTAC instruction counts are printed before and after
/// optimization.
///
/// The output `.wat` file is written next to the source file with the same
/// name and a `.wat` extension.
///
/// # Errors
///
/// Returns an error if any phase fails (IO, parse error, type error, etc.).
/// Error messages include source locations via the [`SourceMap`].
// #[instrument(skip(stage, src))]
pub fn compile(
    stage: Stage,
    dump: bool,
    pipeline: Option<Pipeline>,
    count: bool,
    src: &Path,
) -> anyhow::Result<()> {
    info!(file = %src.to_str().unwrap(), "Starting compilation");
    let input = fs::read_to_string(src)
        .with_context(|| format!("Failed to read source file {}", src.display()))?;
    let lexer = lexer::Lexer::new(&input);

    let file_name = src
        .file_stem()
        .expect("File Name has to exist")
        .to_string_lossy();
    let file_name_ext = src
        .file_name()
        .expect("File Name has to exist")
        .to_string_lossy();

    let dump_dir_name = format!("{}_debug", file_name);
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let dump_dir = parent.join(dump_dir_name);

    if dump {
        fs::create_dir_all(&dump_dir)
            .with_context(|| format!("Failed to create dump dir {}", dump_dir.display()))?;
    }
    let dump_dir = dump_dir.join(file_name_ext.to_string());
    // let tokens = lexer::tokenize(&input);

    // for token in tokens {
    //     match token {
    //         Ok((start, tok, end)) => println!("{}..{}: {:?}", start, end, tok),
    //         Err(e) => println!("Error: {:?}", e),
    //     }
    // }
    let source_map = SourceMap::new(input.clone());
    let mut ast = {
        let _parse_span = span!(Level::INFO, "Parsing").entered();
        let mut parser = parser::Parser::new(lexer);
        parser.parse().map_err(|e| {
            let (line, col) = source_map.get_span_location(e.span());
            anyhow::anyhow!("Parsing failed at {}:{}. {}", line, col, e)
        })?
    };
    if stage == Stage::Parse {
        println!("{:?}", ast);
        return Ok(());
    }

    // Resolve variables and check for duplicates
    {
        crate::log_separator!("Resolving");

        let _resolve_span = span!(Level::INFO, "Resolving").entered();
        let mut resolver = Resolver::new();
        resolver.resolve(&mut ast).map_err(|e| {
            let (line, col) = source_map.get_span_location(&e.span);

            anyhow::anyhow!("Resolving failed at {}:{}. {}", line, col, e.msg)
        })?;
    }
    crate::log_separator!("Typechecking");

    // Typecheck
    let _tcheck_span = span!(Level::INFO, "Typecheck").entered();

    let mut typechecker = Typechecker::new();
    typechecker.typecheck(&ast).map_err(|e| {
        let msg = match e {
            crate::typecheck::TypeError::MismatchedTypes { msg, span, .. }
            | crate::typecheck::TypeError::Generic { msg, span } => {
                let (line, col) = source_map.get_span_location(&span);
                format!("Type Error at {}:{}.  {}", line, col, msg)
            }
        };
        anyhow::anyhow!("Typechecking failed: {}", msg)
    })?;

    drop(_tcheck_span);

    if stage == Stage::Validate {
        return Ok(());
    }

    crate::log_separator!("IR Generation");
    // Wtac generation
    let _wtac_span = span!(Level::INFO, "IR").entered();

    let mut wgen = WtacGen::new(typechecker);

    let w_ast = wgen.generate(ast);

    drop(_wtac_span);

    if dump {
        wtac::wtac_print::write_wtac_file(&dump_dir, &w_ast)
            .with_context(|| format!("Failed to write debug wtac file {}", dump_dir.display()))?;
    }

    if stage == Stage::Wtac {
        println!("{}", w_ast);
        return Ok(());
    }

    if count {
        print_counts("before", &w_ast);
    }

    // An empty pipeline still round-trips through the CFG, so `--passes ''` is a
    // fair baseline to measure a single pass against.
    let w_ast = match pipeline {
        Some(pipeline) => {
            crate::log_separator!("Optimization");

            let _opt_span = span!(Level::INFO, "Optimization").entered();
            let w_ast_opt = optimizer::optmize(w_ast, dump, &dump_dir, &pipeline);
            let opt_path = crate::util::extend_file_name(&dump_dir, "_opt").unwrap();

            if dump {
                wtac::wtac_print::write_wtac_file(&opt_path, &w_ast_opt).with_context(|| {
                    format!("Failed to write debug opt wtac file {}", opt_path.display())
                })?;
            }
            drop(_opt_span);
            if count {
                print_counts("after", &w_ast_opt);
            }
            w_ast_opt
        }
        _ => w_ast,
    };

    {
        crate::log_separator!("Emitting WASM");

        let _emitter_span = span!(Level::INFO, "Emitting WASM").entered();

        // Emit
        let wasm_path = src.with_extension("wat");
        let file = fs::File::create(wasm_path).unwrap();
        let buf_writer = BufWriter::new(file);
        let mut emitter = emitter::Emitter::new(buf_writer);
        let _ = emitter.emit(w_ast);
    }
    Ok(())
}

/// Prints per-function WTAC instruction counts for `--count`.
fn print_counts(label: &str, prog: &crate::wtac::wtac_ast::Program) {
    let counts = crate::wtac::wtac_ast::function_counts(prog);
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    println!("wtac instructions ({label}):");
    for (name, n) in &counts {
        println!("  {name:<24} {n:>6}");
    }
    println!("  {:<24} {total:>6}", "TOTAL");
}
