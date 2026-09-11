//! Record layout.
//!
//! Eta records use natural alignment, the usual C / System V rule:
//!
//! - a field starts at the next offset that is a multiple of its own alignment
//! - the record's alignment is the widest alignment of any field
//! - the record's size is the end of the last field, rounded up to that
//!   alignment, so that records stay aligned when placed back to back
//!
//! Under wasm32: `int` is 8 bytes aligned to 8, `bool` is 4 aligned to 4, and
//! an array or record field is a 4-byte pointer aligned to 4.
//!
//! These tests are the specification for that layout. They are also the
//! reference suite for the records project, so they assert offsets and sizes
//! directly rather than snapshotting whatever the compiler produced.

use rwetac::{lexer, parser, resolver::Resolver, typecheck::Typechecker};

struct Layout {
    fields: Vec<(String, usize)>,
    align: usize,
    size: usize,
}

fn layout_of(src: &str, tag: &str) -> Layout {
    let lexer = lexer::Lexer::new(src);
    let mut parser = parser::Parser::new(lexer);
    let mut ast = parser.parse_program().expect("parse failed");
    if let Err(e) = Resolver::new().resolve(&mut ast) {
        panic!("resolve failed: {}", e.msg);
    }
    let mut tchecker = Typechecker::new();
    tchecker.typecheck(&ast).expect("typecheck failed");

    let rec = tchecker.symtab.get_record(tag);
    Layout {
        fields: rec
            .members
            .iter()
            .map(|(name, m)| (name.clone(), m.offset))
            .collect(),
        align: rec.align,
        size: rec.size(),
    }
}

/// Wraps a record declaration in a program the typechecker will accept.
fn program(decl: &str) -> String {
    format!("{decl}\n\nmain() : int {{\n    return 0\n}}\n")
}

#[test]
fn all_int_fields_need_no_padding() {
    let l = layout_of(&program("record P {\n    x : int\n    y : int\n}"), "P");
    assert_eq!(l.fields, [("x".into(), 0), ("y".into(), 8)]);
    assert_eq!(l.align, 8);
    assert_eq!(l.size, 16);
}

#[test]
fn all_bool_fields_pack_at_four() {
    let l = layout_of(&program("record S {\n    a : bool\n    b : bool\n}"), "S");
    assert_eq!(l.fields, [("a".into(), 0), ("b".into(), 4)]);
    assert_eq!(l.align, 4);
    assert_eq!(l.size, 8);
}

#[test]
fn a_bool_before_an_int_is_padded_to_eight() {
    // b cannot sit at 4, so 4 bytes of padding go in front of it.
    let l = layout_of(&program("record Q {\n    a : bool\n    b : int\n}"), "Q");
    assert_eq!(l.fields, [("a".into(), 0), ("b".into(), 8)]);
    assert_eq!(l.align, 8);
    assert_eq!(l.size, 16);
}

#[test]
fn a_trailing_bool_still_costs_a_full_slot() {
    // The last field ends at 12, but the record is 8-aligned, so it is 16.
    // This tail padding is what keeps an array of records aligned.
    let l = layout_of(&program("record R {\n    a : int\n    b : bool\n}"), "R");
    assert_eq!(l.fields, [("a".into(), 0), ("b".into(), 8)]);
    assert_eq!(l.align, 8);
    assert_eq!(l.size, 16);
}

#[test]
fn an_array_field_is_a_four_byte_pointer() {
    let l = layout_of(&program("record U {\n    a : int[]\n    b : int\n}"), "U");
    assert_eq!(l.fields, [("a".into(), 0), ("b".into(), 8)]);
    assert_eq!(l.align, 8);
    assert_eq!(l.size, 16);
}

#[test]
fn field_order_changes_the_size() {
    // The classic demonstration, and the one the exam question asks about:
    // the same three fields cost 24 bytes in one order and 16 in another.
    let bad = layout_of(
        &program("record T {\n    a : bool\n    b : int\n    c : bool\n}"),
        "T",
    );
    assert_eq!(
        bad.fields,
        [("a".into(), 0), ("b".into(), 8), ("c".into(), 16)]
    );
    assert_eq!(bad.size, 24);

    let good = layout_of(
        &program("record T {\n    b : int\n    a : bool\n    c : bool\n}"),
        "T",
    );
    assert_eq!(
        good.fields,
        [("b".into(), 0), ("a".into(), 8), ("c".into(), 12)]
    );
    assert_eq!(good.size, 16);
}

#[test]
fn fields_sharing_a_type_lay_out_like_separate_ones() {
    // `x, y : int` is two fields, not one.
    let shared = layout_of(&program("record P {\n    x, y : int\n}"), "P");
    let separate = layout_of(&program("record P {\n    x : int\n    y : int\n}"), "P");
    assert_eq!(shared.fields, separate.fields);
    assert_eq!(shared.size, separate.size);
}
