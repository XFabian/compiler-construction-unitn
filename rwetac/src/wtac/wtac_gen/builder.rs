//! [`InstrBuilder`], which accumulates instructions and mints the temporaries
//! they write to.
//!
//! # Design decision: pointers
//!
//! WASM has no pointer type and none is needed here. Pointers are 32-bit and all
//! pointer arithmetic happens in `I32`, so an address is just a `Value` whose
//! `WType` is `I32`, indistinguishable from any other 32-bit integer.
//!
//! That is why every helper below takes the `WType` it should operate at instead
//! of inferring one from a source-level type: the same `binary`, `copy` and
//! `ptr_offset` calls serve `I64` arithmetic and address computation alike.

use super::*;

pub(super) struct InstrBuilder<'a> {
    pub(super) gene: &'a mut WtacGen,
    instrs: Vec<W::Instruction>,
}

impl<'a> InstrBuilder<'a> {
    pub(super) fn new(gene: &'a mut WtacGen) -> Self {
        InstrBuilder {
            gene,
            instrs: Vec::new(),
        }
    }

    /// `dst = src1 op src2`, returns dst
    pub(super) fn binary(
        &mut self,
        op: W::BinaryOp,
        src1: W::Value,
        src2: W::Value,
        wt: W::WType,
    ) -> W::Value {
        let dst = self.gene.make_wtac_var(wt.clone());
        self.instrs.push(W::Instruction::Binary {
            op,
            src1,
            src2,
            dst: dst.clone(),
            t: wt,
        });
        dst
    }

    /// `dst = src1 op src2` into an existing destination.
    pub(super) fn binary_to(
        &mut self,
        op: W::BinaryOp,
        src1: W::Value,
        src2: W::Value,
        dst: W::Value,
        wt: W::WType,
    ) {
        self.instrs.push(W::Instruction::Binary {
            op,
            src1,
            src2,
            dst,
            t: wt,
        });
    }

    /// `dst: bool = src1 cmp src2` — comparison on operands of type `operand_ty`,
    /// result is always `Bool`.
    pub(super) fn compare(
        &mut self,
        op: W::BinaryOp,
        src1: W::Value,
        src2: W::Value,
        operand_wt: W::WType,
    ) -> W::Value {
        // The result is a bool (i32) whatever the operands were.
        let dst = self.gene.make_wtac_var(W::WType::I32);
        self.instrs.push(W::Instruction::Binary {
            op,
            src1,
            src2,
            dst: dst.clone(),
            t: operand_wt,
        });
        dst
    }

    /// `dst = op src`, returns dst
    pub(super) fn unary(&mut self, op: W::UnaryOp, src: W::Value, wt: W::WType) -> W::Value {
        let dst = self.gene.make_wtac_var(wt.clone());
        self.instrs.push(W::Instruction::Unary {
            op,
            src,
            dst: dst.clone(),
            t: wt,
        });
        dst
    }

    /// `dst = src`, returns dst
    pub(super) fn copy(&mut self, src: W::Value, wt: W::WType) -> W::Value {
        let dst = self.gene.make_wtac_var(wt.clone());
        self.instrs.push(W::Instruction::Copy {
            src,
            dst: dst.clone(),
            t: wt,
        });
        dst
    }

    /// `dst = src` into a specific destination (no temp created)
    pub(super) fn copy_to(&mut self, src: W::Value, dst: W::Value, wt: W::WType) {
        self.instrs.push(W::Instruction::Copy { src, dst, t: wt });
    }

    /// `dst = name(args)`, returns dst
    pub(super) fn call(&mut self, name: &str, args: Vec<W::Value>, ret_wt: W::WType) -> W::Value {
        let dst = self.gene.make_wtac_var(ret_wt);
        self.instrs.push(W::Instruction::FCall {
            name: name.to_string(),
            args,
            dst: Some(dst.clone()),
        });
        dst
    }

    /// Wraps an i64 value to i32 (for pointer arithmetic)
    pub(super) fn wrap(&mut self, src: W::Value) -> W::Value {
        self.unary(W::UnaryOp::Wrap, src, W::WType::I32)
    }

    /// Convenience: `base + (index * elem_size)`, wrapped to i32
    pub(super) fn ptr_offset(
        &mut self,
        base: &W::Value,
        index: &W::Value,
        elem_ty: &Type,
    ) -> W::Value {
        let scaled = self.binary(
            W::BinaryOp::Mult,
            index.clone(),
            W::Value::Imm(types::get_size(elem_ty) as i64, W::WType::I64),
            W::WType::I64,
        );
        let wrapped = self.wrap(scaled);
        self.binary(W::BinaryOp::Add, base.clone(), wrapped, W::WType::I32)
    }

    pub(super) fn brif(&mut self, cond: &W::Value, lbl: &str) {
        self.instrs.push(W::Instruction::BrIf {
            cond: cond.clone(),
            l: lbl.to_string(),
        });
    }

    pub(super) fn br(&mut self, lbl: &str) {
        self.instrs.push(W::Instruction::Br(lbl.to_string()));
    }

    /// Creates an i64 immediate constant.
    pub(super) fn i64_const(&self, val: i64) -> W::Value {
        W::Value::Imm(val, W::WType::I64)
    }

    /// Creates an i32 immediate constant.
    pub(super) fn i32_const(&self, val: i64) -> W::Value {
        W::Value::Imm(val, W::WType::I32)
    }

    pub(super) fn extend(&mut self, elem: Vec<W::Instruction>) {
        self.instrs.extend(elem);
    }

    /// Consumes the builder, returning accumulated instructions
    pub(super) fn finish(self) -> Vec<W::Instruction> {
        self.instrs
    }
}
