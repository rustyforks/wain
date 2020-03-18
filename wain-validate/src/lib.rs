// Module validation for Wasm module
// - https://webassembly.github.io/spec/core/valid/index.html
// - https://webassembly.github.io/spec/core/appendix/algorithm.html#algo-valid
extern crate wain_ast;

mod error;
mod insn;

use error::{Error, ErrorKind, Result};
use wain_ast::*;

// Validation context
// https://webassembly.github.io/spec/core/valid/conventions.html#context
struct Context<'module, 'a: 'module> {
    module: &'module Module<'a>,
    source: &'a str,
}

impl<'module, 'a> Context<'module, 'a> {
    fn error<T>(&self, kind: ErrorKind, offset: usize) -> Result<'a, T> {
        Err(Error::new(kind, offset, self.source))
    }

    fn validate_idx<T>(
        &self,
        s: &'module [T],
        idx: u32,
        what: &'static str,
        offset: usize,
    ) -> Result<'a, &'module T> {
        if let Some(item) = s.get(idx as usize) {
            Ok(item)
        } else {
            self.error(
                ErrorKind::IndexOutOfBounds {
                    idx,
                    upper: s.len(),
                    what,
                },
                offset,
            )
        }
    }

    fn type_from_idx(&self, idx: u32, offset: usize) -> Result<'a, &'module FuncType> {
        self.validate_idx(&self.module.types, idx, "type", offset)
    }

    fn func_from_idx(&self, idx: u32, offset: usize) -> Result<'a, &'module Func> {
        self.validate_idx(&self.module.funcs, idx, "function", offset)
    }

    fn table_from_idx(&self, idx: u32, offset: usize) -> Result<'a, &'module Table> {
        self.validate_idx(&self.module.tables, idx, "table", offset)
    }

    fn global_from_idx(&self, idx: u32, offset: usize) -> Result<'a, &'module Global> {
        self.validate_idx(&self.module.globals, idx, "global variable", offset)
    }
}

pub fn validate<'module, 'a>(module: &'module Module<'a>, source: &'a str) -> Result<'a, ()> {
    let mut ctx = Context {
        module: &module,
        source,
    };
    module.validate(&mut ctx)
}

trait Validate<'a> {
    fn validate<'module>(&self, ctx: &mut Context<'module, 'a>) -> Result<'a, ()>;
}

impl<'a, V: Validate<'a>> Validate<'a> for Vec<V> {
    fn validate<'module>(&self, ctx: &mut Context<'module, 'a>) -> Result<'a, ()> {
        self.iter().map(|n| n.validate(ctx)).collect()
    }
}

impl<'a, V: Validate<'a>> Validate<'a> for Option<V> {
    fn validate<'module>(&self, ctx: &mut Context<'module, 'a>) -> Result<'a, ()> {
        match self {
            Some(node) => node.validate(ctx),
            None => Ok(()),
        }
    }
}

// https://webassembly.github.io/spec/core/valid/modules.html#valid-module
impl<'a> Validate<'a> for Module<'a> {
    fn validate<'module>(&self, ctx: &mut Context<'module, 'a>) -> Result<'a, ()> {
        self.types.validate(ctx)?;
        // TODO: Check tables[0]
        // TODO: Check memories[0]
        self.funcs.validate(ctx)?;
        Ok(())
    }
}

// https://webassembly.github.io/spec/core/valid/types.html#valid-functype
impl<'a> Validate<'a> for FuncType {
    fn validate<'module>(&self, ctx: &mut Context<'module, 'a>) -> Result<'a, ()> {
        if self.results.len() > 1 {
            ctx.error(
                ErrorKind::MultipleReturnTypes(self.results.clone()),
                self.start,
            )
        } else {
            Ok(())
        }
    }
}

// https://webassembly.github.io/spec/core/valid/modules.html#imports
// Not implement Validate<'a> since the offset parameter is necessary for better error message
fn validate_import<'module, 'a>(
    import: &Import<'a>,
    ctx: &mut Context<'module, 'a>,
    offset: usize,
) -> Result<'a, ()> {
    if import.mod_name.0 != "env" && import.name.0 != "print" {
        let mod_name = import.mod_name.0.to_string();
        let name = import.name.0.to_string();
        ctx.error(ErrorKind::UnknownImport { mod_name, name }, offset)
    } else {
        Ok(())
    }
}

// https://webassembly.github.io/spec/core/valid/modules.html#functions
impl<'a> Validate<'a> for Func<'a> {
    fn validate<'module>(&self, ctx: &mut Context<'module, 'a>) -> Result<'a, ()> {
        let func_ty = ctx.type_from_idx(self.idx, self.start)?;
        match &self.kind {
            FuncKind::Import(import) => validate_import(import, ctx, self.start),
            FuncKind::Body { locals, expr } => {
                if locals.len() < func_ty.params.len() {
                    return ctx.error(
                        ErrorKind::TooFewFuncLocalsForParams {
                            params: func_ty.params.len(),
                            locals: locals.len(),
                        },
                        self.start,
                    );
                }
                for (i, param) in func_ty.params.iter().enumerate() {
                    let local = locals[i];
                    if local != *param {
                        return ctx.error(
                            ErrorKind::ParamTypeMismatchWithLocal {
                                idx: i,
                                param: *param,
                                local,
                            },
                            self.start,
                        );
                    }
                }

                // FuncType validated func_ty has at most one result type
                let ret = func_ty.results.get(0).copied();
                crate::insn::validate_func_body(expr, locals, ret, ctx, self.start)
            }
        }
    }
}