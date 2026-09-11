use std::{fs, path::Path};

use rwetac::{lexer, parser, resolver, typecheck};

fn run_invalid_case(src: &str) -> String {
    let lexer = lexer::Lexer::new(src);
    let mut parser = parser::Parser::new(lexer);
    match parser.parse_program() {
        Ok(_) => panic!("Parsing succeeded unexpectedly"),
        Err(e) => format!("Failure: {}", e),
    }
}

fn run_invalid_case_type(src: &str) -> String {
    let lexer = lexer::Lexer::new(src);
    let mut parser = parser::Parser::new(lexer);
    let mut resolver = resolver::Resolver::new();
    let mut tchecker = typecheck::Typechecker::new();
    let mut ast = parser.parse_program().unwrap();
    match resolver.resolve(&mut ast) {
        Ok(_) => (),
        Err(e) => return format!("Failure: {}", e.msg),
    }
    match tchecker.typecheck(&ast) {
        Ok(_) => panic!("Typechecking succeeded unexpectedly"),
        Err(e) => {
            let msg = match e {
                typecheck::TypeError::MismatchedTypes { msg, .. } => msg,
                typecheck::TypeError::Generic { msg, .. } => msg,
            };
            format!("Failure: {}", msg)
        }
    }
}

#[test]
fn invalid_parse_cases() {
    let dir = Path::new("tests/inputs/invalid");
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(path).unwrap();
        let out = run_invalid_case(&src);
        // let snapshot_name = format!("invalid_parse_case_{}", name);
        insta::with_settings!({
            snapshot_suffix => name
        },
        {
            insta::assert_snapshot!(out);
        }
        );
    }
    // panic!("b")
}

#[test]
fn invalid_typecheck_cases() {
    let dir = Path::new("tests/inputs/invalid_semantic");
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(path).unwrap();
        let out = run_invalid_case_type(&src);
        // let snapshot_name = format!("invalid_parse_case_{}", name);
        insta::with_settings!({
            snapshot_suffix => name
        },
        {
            insta::assert_snapshot!(out);
        }
        );
    }
    // panic!("b")
}

#[test]
fn valid_cases() {
    let dir = Path::new("tests/inputs/");
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            continue;
        }
        if path.extension().unwrap() != "eta" {
            continue;
        }
        println!("{:?}", path);
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let wat_path = path.with_extension("wat");
        // Both settings: the optimizer has emitted invalid WASM before, and
        // checking only the unoptimized build is what hid it.
        for opt in [false, true] {
            let pipeline = opt.then(rwetac::setting::Pipeline::default_full);
            if let Err(e) = rwetac::compile::compile(
                rwetac::setting::Stage::Codegen,
                false,
                pipeline,
                false,
                &path,
            ) {
                panic!("{name} (opt={opt}): compilation failed: {e:?}");
            }
            let wat_code = fs::read_to_string(&wat_path).unwrap();
            fs::remove_file(&wat_path).unwrap();
            // Asserted, not snapshotted: a snapshot of a fixed success string lets
            // `cargo insta accept` bless a validation failure as the expectation.
            if let Err(e) = wat::parse_str(&wat_code) {
                panic!("{name} (opt={opt}): generated wat failed to validate: {e:#?}");
            }
        }
    }
}
