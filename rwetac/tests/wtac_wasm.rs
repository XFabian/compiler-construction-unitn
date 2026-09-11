// Snapshot test for operations in wtac to wasm
// Like store and loads

use insta::assert_snapshot;
mod common;

use rwetac::symbols::{MemberEntry, RecordEntry};
use rwetac::typecheck::Typechecker;
use rwetac::types::Type;
use rwetac::wtac::wtac_ast::*;
use rwetac::wtac::wtac_gen::WtacGen;
use rwetac::{self, emitter::Emitter};
use rwetac::{lexer, parser};

// These tests are for me. There may be other ways to represent this
pub fn setup(instr: Vec<Instruction>) -> Program {
    let fun = TopLevel::Function {
        name: "test".to_string(),
        body: instr,
        params: vec![],
        f_type: WType::FunType {
            param_type: vec![],
            ret_type: None,
        },
        locals: vec![
            ("x".to_string(), WType::I64),
            ("y".to_string(), WType::I64),
            ("z".to_string(), WType::I64),
            ("a".to_string(), WType::I32),
            ("b".to_string(), WType::I32),
        ],
    };
    Program { decls: vec![fun] }
}
#[test]
fn wtac_mem_load_wasm() {
    let mut out = Vec::new();
    let instruction = Instruction::Copy {
        src: Value::Mem {
            name: "a".to_string(),
            offset: 8,
            t: WType::I64,
        },
        dst: Value::Var("x".to_string()),
        t: WType::I64,
    };
    let prog = setup(vec![instruction]);
    let mut emitter = Emitter::new(&mut out);

    let _ = emitter.emit(prog);
    let out_str = String::from_utf8(out).unwrap();
    assert_snapshot!(out_str, @r#"
    (module
    (import "env" "eta_malloc" (func $eta_malloc (param i32) (result i32)))
    (memory (export "memory") 17)
    (func $test(local $x i64)
        (local $y i64)
        (local $z i64)
        (local $a i32)
        (local $b i32)
        local.get $a
        i32.const 8
        i32.add
        i64.load
        local.set $x
    )

    (global $__stack_pointer (mut i32) (i32.const 66636) )
    (global $__data_end i32 (i32.const 100) )
    (global $__heap_base (export "__heap_base") (mut i32) (i32.const 66636) )
    (export "test" (func $test))
    )
    "#);
}

#[test]
fn wtac_mem_store_wasm() {
    let instruction = Instruction::Copy {
        src: Value::Var("x".to_string()),
        dst: Value::Mem {
            name: "a".to_string(),
            offset: 8,
            t: WType::I64,
        },
        t: WType::I64,
    };
    let prog = setup(vec![instruction]);
    let mut out = Vec::new();
    let mut emitter = Emitter::new(&mut out);

    let _ = emitter.emit(prog);
    let out_str = String::from_utf8(out).unwrap();
    assert_snapshot!(out_str, @r#"
    (module
    (import "env" "eta_malloc" (func $eta_malloc (param i32) (result i32)))
    (memory (export "memory") 17)
    (func $test(local $x i64)
        (local $y i64)
        (local $z i64)
        (local $a i32)
        (local $b i32)
        local.get $a
        i32.const 8
        i32.add
        local.get $x
        i64.store
    )

    (global $__stack_pointer (mut i32) (i32.const 66636) )
    (global $__data_end i32 (i32.const 100) )
    (global $__heap_base (export "__heap_base") (mut i32) (i32.const 66636) )
    (export "test" (func $test))
    )
    "#);
}

#[test]
fn rec_load_store() {
    let input = r#"newPoint() : Point { return Point(1,true)
    }"#;
    let lexer = lexer::Lexer::new(input);
    let mut parser = parser::Parser::new(lexer);
    let ast = parser.parse().unwrap();

    let mut tchecker = Typechecker::new();
    let members = vec![
        (
            "x".to_string(),
            MemberEntry {
                member_t: Type::Int,
                offset: 0,
            },
        ),
        (
            "y".to_string(),
            MemberEntry {
                member_t: Type::Bool,
                offset: 8,
            },
        ),
    ];
    let rec_entry = RecordEntry { members, align: 8 };
    tchecker.symtab.add_record("Point".to_string(), rec_entry);

    tchecker.typecheck(&ast).unwrap();

    let mut wgen = WtacGen::new(tchecker);
    let wtac_ast = wgen.generate(ast);
    let mut out = Vec::new();
    let mut emitter = Emitter::new(&mut out);
    let _ = emitter.emit(wtac_ast);
    let out_str = String::from_utf8(out).unwrap();
    let res = match wat::parse_str(&out_str) {
        Ok(_) => "wat2wasm typecheck Ok".to_string(),
        Err(e) => format!("Failure: {:#?}", e),
    };
    assert_snapshot!(res, @"wat2wasm typecheck Ok");
}
