// Each integration test binary compiles this module separately and uses only
// part of it, so anything unused from one binary's point of view looks dead.
#![allow(dead_code)]

use rwetac::{
    lexer, parser,
    symbols::{MemberEntry, RecordEntry},
    typecheck::Typechecker,
    types::Type,
};

pub struct Env<'a> {
    pub parser: parser::Parser<'a>,
    pub tchecker: Typechecker,
}
pub fn setup(input: &str) -> Env<'_> {
    let lexer = lexer::Lexer::new(input);
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
                member_t: Type::Int,
                offset: 0,
            },
        ),
    ];

    // RecordEntry'
    let rec_entry = RecordEntry { members, align: 8 };
    tchecker.symtab.add_record("Point".to_string(), rec_entry);
    tchecker
        .symtab
        .add_local_var("p".to_string(), Type::Record("Point".to_string()));

    // Some more bindings that can be used in tests
    let f_type = Type::FunType {
        param_types: vec![Type::Int, Type::Int],
        ret_type: vec![Type::Int, Type::Int],
    };
    tchecker.symtab.add_fun("test".to_string(), f_type, true);
    tchecker.symtab.add_local_var("x".to_string(), Type::Int);
    tchecker.symtab.add_local_var("y".to_string(), Type::Int);
    tchecker.symtab.add_local_var("z".to_string(), Type::Int);
    tchecker.symtab.add_local_var(
        "a".to_string(),
        Type::Array {
            elem_type: Box::new(Type::Int),
        },
    );
    tchecker.symtab.add_local_var(
        "b".to_string(),
        Type::Array {
            elem_type: Box::new(Type::Bool),
        },
    );
    Env {
        parser: parser::Parser::new(lexer),
        tchecker,
    }
}

pub fn typecheck_stmt(input: &str) -> String {
    let mut env = setup(input);
    let ast = env.parser.parse_statement().unwrap();
    match env.tchecker.typecheck_statement(&ast, &Type::Bool) {
        Ok(_) => panic!("Typechecking succeded unexpectedly"),
        Err(e) => format!("Failure: {:#?}", e),
    }
}

pub fn typecheck_expr(input: &str) -> String {
    let mut env = setup(input);
    let ast = env.parser.parse_expression(0).unwrap();
    match env.tchecker.typecheck_expression(&ast) {
        Ok(_) => panic!("Typechecking succeded unexpectedly"),
        Err(e) => format!("Failure: {:#?}", e),
    }
}

pub fn typecheck_expr_succeed(input: &str) -> String {
    let mut env = setup(input);
    let ast = env.parser.parse_expression(0).unwrap();
    match env.tchecker.typecheck_expression(&ast) {
        Ok(t) => format!("{}", t),
        Err(_) => panic!("Typechecking failed unexpectedly"),
    }
}

pub fn typecheck_stmt_succeed(input: &str) -> String {
    let mut env = setup(input);
    let ast = env.parser.parse_statement().unwrap();
    match env.tchecker.typecheck_statement(&ast, &Type::Bool) {
        Ok(_) => "Ok(())".to_string(),
        Err(_) => panic!("Typechecking failed unexpectedly"),
    }
}
