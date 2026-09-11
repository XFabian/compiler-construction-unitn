//! Compiler configuration.
//!
//! Defines the [`Stage`] enum that controls how far the compilation pipeline runs
//! and the [`Pipeline`] of optimization [`Pass`]es to apply.
//!
//! [`Stage`] is selected via the `--parse`, `--validate`, `--wtac`, or `--codegen`
//! command-line flags; the pipeline via `--passes`.

use std::fmt;

/// Controls which compilation stage to stop after.
///
/// Each stage includes all previous stages. For example, [`Validate`](Stage::Validate)
/// will lex, parse, resolve, and typecheck, then stop.
/// If no stage is specified, the compiler defaults to [`Codegen`](Stage::Codegen).
#[derive(Debug, PartialEq)]
pub enum Stage {
    Parse,
    Validate,
    Wtac,
    Codegen,
}

/// A single optimization pass.
///
/// The short names are the ones accepted by `--passes` and printed by
/// `--report-passes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Constant propagation.
    Cp,
    /// Copy propagation.
    Copy,
    /// Constant folding.
    Cf,
    /// Dead code elimination.
    Dce,
    /// Pointer propagation.
    Ptr,
    /// Store-to-load forwarding.
    Stl,
}

impl Pass {
    /// Every pass this compiler implements, in the order the default pipeline
    /// runs them.
    pub const ALL: [Pass; 6] = [
        Pass::Cp,
        Pass::Copy,
        Pass::Cf,
        Pass::Dce,
        Pass::Ptr,
        Pass::Stl,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Pass::Cp => "cp",
            Pass::Copy => "copy",
            Pass::Cf => "cf",
            Pass::Dce => "dce",
            Pass::Ptr => "ptr",
            Pass::Stl => "stl",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Pass::Cp => "constant propagation",
            Pass::Copy => "copy propagation",
            Pass::Cf => "constant folding",
            Pass::Dce => "dead code elimination",
            Pass::Ptr => "pointer propagation",
            Pass::Stl => "store-to-load forwarding",
        }
    }

    pub fn from_name(name: &str) -> Result<Self, String> {
        Pass::ALL
            .into_iter()
            .find(|p| p.name() == name)
            .ok_or_else(|| {
                let known: Vec<_> = Pass::ALL.iter().map(|p| p.name()).collect();
                format!("unknown pass `{name}`. Known passes: {}", known.join(", "))
            })
    }
}

impl fmt::Display for Pass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// One entry in a [`Pipeline`]: either a single pass, or a group run repeatedly
/// until none of them reports a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineItem {
    Pass(Pass),
    Fixpoint(Vec<Pass>),
}

/// An ordered optimization pipeline.
///
/// Parsed from `--passes`, e.g. `cf,dce` or `fixpoint(cp,copy,cf,dce)`. Order
/// matters and is the user's to choose — a pass placed before the one that
/// feeds it will simply find nothing to do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pipeline(pub Vec<PipelineItem>);

impl Pipeline {
    /// Every pass, run to a fixed point. This is what `--opt` selects and what
    /// the compiler did before pipelines were configurable.
    pub fn default_full() -> Self {
        Pipeline(vec![PipelineItem::Fixpoint(Pass::ALL.to_vec())])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Parses a `--passes` specification.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut items = Vec::new();
        for part in split_top_level(spec)? {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part
                .strip_prefix("fixpoint(")
                .and_then(|r| r.strip_suffix(')'))
            {
                Some(inner) => {
                    let passes = inner
                        .split(',')
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .map(Pass::from_name)
                        .collect::<Result<Vec<_>, _>>()?;
                    if passes.is_empty() {
                        return Err("`fixpoint(...)` needs at least one pass".to_string());
                    }
                    items.push(PipelineItem::Fixpoint(passes));
                }
                None => items.push(PipelineItem::Pass(Pass::from_name(part)?)),
            }
        }
        Ok(Pipeline(items))
    }
}

impl fmt::Display for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .0
            .iter()
            .map(|item| match item {
                PipelineItem::Pass(p) => p.name().to_string(),
                PipelineItem::Fixpoint(ps) => {
                    let names: Vec<_> = ps.iter().map(|p| p.name()).collect();
                    format!("fixpoint({})", names.join(","))
                }
            })
            .collect();
        write!(f, "{}", parts.join(","))
    }
}

/// Splits on commas that are not inside parentheses, so `fixpoint(a,b)` stays
/// one part.
fn split_top_level(s: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "unbalanced `)` in pass list".to_string())?
            }
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => (),
        }
    }
    if depth != 0 {
        return Err("unbalanced `(` in pass list".to_string());
    }
    out.push(&s[start..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_flat_list() {
        assert_eq!(
            Pipeline::parse("cf,dce").unwrap(),
            Pipeline(vec![
                PipelineItem::Pass(Pass::Cf),
                PipelineItem::Pass(Pass::Dce)
            ])
        );
    }

    #[test]
    fn parses_a_fixpoint_group() {
        assert_eq!(
            Pipeline::parse("fixpoint(cp,dce)").unwrap(),
            Pipeline(vec![PipelineItem::Fixpoint(vec![Pass::Cp, Pass::Dce])])
        );
    }

    #[test]
    fn parses_a_mixed_pipeline() {
        assert_eq!(
            Pipeline::parse("cf,fixpoint(cp,copy),dce").unwrap(),
            Pipeline(vec![
                PipelineItem::Pass(Pass::Cf),
                PipelineItem::Fixpoint(vec![Pass::Cp, Pass::Copy]),
                PipelineItem::Pass(Pass::Dce),
            ])
        );
    }

    #[test]
    fn round_trips_through_display() {
        let spec = "cf,fixpoint(cp,copy),dce";
        assert_eq!(Pipeline::parse(spec).unwrap().to_string(), spec);
    }

    #[test]
    fn rejects_an_unknown_pass() {
        let err = Pipeline::parse("licm").unwrap_err();
        assert!(err.contains("unknown pass `licm`"), "{err}");
    }

    #[test]
    fn rejects_unbalanced_parens() {
        assert!(Pipeline::parse("fixpoint(cp").is_err());
        assert!(Pipeline::parse("cp)").is_err());
    }

    #[test]
    fn rejects_an_empty_fixpoint() {
        assert!(Pipeline::parse("fixpoint()").is_err());
    }

    #[test]
    fn an_empty_spec_is_an_empty_pipeline() {
        assert!(Pipeline::parse("").unwrap().is_empty());
    }
}
