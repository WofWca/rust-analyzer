//! FIXME: write short doc here

mod atom;

pub(crate) use self::atom::match_arm_list;
pub(super) use self::atom::{literal, LITERAL_FIRST};
use super::*;

pub(super) enum StmtWithSemi {
    Yes,
    No,
    Optional,
}

const EXPR_FIRST: TokenSet = LHS_FIRST;

pub(super) fn expr(p: &mut Parser) -> BlockLike {
    let r = Restrictions { forbid_structs: false, prefer_stmt: false };
    expr_bp(p, r, 1).1
}

pub(super) fn expr_stmt(p: &mut Parser) -> (Option<CompletedMarker>, BlockLike) {
    let r = Restrictions { forbid_structs: false, prefer_stmt: true };
    expr_bp(p, r, 1)
}

fn expr_no_struct(p: &mut Parser) {
    let r = Restrictions { forbid_structs: true, prefer_stmt: false };
    expr_bp(p, r, 1);
}

// test block
// fn a() {}
// fn b() { let _ = 1; }
// fn c() { 1; 2; }
// fn d() { 1; 2 }
pub(crate) fn block(p: &mut Parser) {
    if !p.at(T!['{']) {
        p.error("expected a block");
        return;
    }
    atom::block_expr(p, None);
}

pub(crate) fn naked_block(p: &mut Parser) {
    assert!(p.at(T!['{']));
    let m = p.start();
    p.bump(T!['{']);
    expr_block_contents(p);
    p.expect(T!['}']);
    m.complete(p, BLOCK);
}

fn is_expr_stmt_attr_allowed(kind: SyntaxKind) -> bool {
    match kind {
        BIN_EXPR | RANGE_EXPR | IF_EXPR => false,
        _ => true,
    }
}

pub(super) fn stmt(p: &mut Parser, with_semi: StmtWithSemi) {
    // test block_items
    // fn a() { fn b() {} }
    let m = p.start();
    // test attr_on_expr_stmt
    // fn foo() {
    //     #[A] foo();
    //     #[B] bar!{}
    //     #[C] #[D] {}
    //     #[D] return ();
    // }
    let has_attrs = p.at(T![#]);
    attributes::outer_attributes(p);

    if p.at(T![let]) {
        let_stmt(p, m, with_semi);
        return;
    }

    let m = match items::maybe_item(p, m, items::ItemFlavor::Mod) {
        Ok(()) => return,
        Err(m) => m,
    };

    let (cm, blocklike) = expr_stmt(p);
    let kind = cm.as_ref().map(|cm| cm.kind()).unwrap_or(ERROR);

    if has_attrs && !is_expr_stmt_attr_allowed(kind) {
        // test_err attr_on_expr_not_allowed
        // fn foo() {
        //    #[A] 1 + 2;
        //    #[B] if true {};
        // }
        p.error(format!("attributes are not allowed on {:?}", kind));
    }

    if p.at(T!['}']) {
        // test attr_on_last_expr_in_block
        // fn foo() {
        //     { #[A] bar!()? }
        //     #[B] &()
        // }
        if let Some(cm) = cm {
            cm.undo_completion(p).abandon(p);
            m.complete(p, kind);
        } else {
            m.abandon(p);
        }
    } else {
        // test no_semi_after_block
        // fn foo() {
        //     if true {}
        //     loop {}
        //     match () {}
        //     while true {}
        //     for _ in () {}
        //     {}
        //     {}
        //     macro_rules! test {
        //          () => {}
        //     }
        //     test!{}
        // }

        match with_semi {
            StmtWithSemi::Yes => {
                if blocklike.is_block() {
                    p.eat(T![;]);
                } else {
                    p.expect(T![;]);
                }
            }
            StmtWithSemi::No => {}
            StmtWithSemi::Optional => {
                if p.at(T![;]) {
                    p.eat(T![;]);
                }
            }
        }

        m.complete(p, EXPR_STMT);
    }

    // test let_stmt
    // fn foo() {
    //     let a;
    //     let b: i32;
    //     let c = 92;
    //     let d: i32 = 92;
    //     let e: !;
    //     let _: ! = {};
    // }
    fn let_stmt(p: &mut Parser, m: Marker, with_semi: StmtWithSemi) {
        assert!(p.at(T![let]));
        p.bump(T![let]);
        patterns::pattern(p);
        if p.at(T![:]) {
            types::ascription(p);
        }
        if p.eat(T![=]) {
            expressions::expr(p);
        }

        match with_semi {
            StmtWithSemi::Yes => {
                p.expect(T![;]);
            }
            StmtWithSemi::No => {}
            StmtWithSemi::Optional => {
                if p.at(T![;]) {
                    p.eat(T![;]);
                }
            }
        }
        m.complete(p, LET_STMT);
    }
}

pub(crate) fn expr_block_contents(p: &mut Parser) {
    // This is checked by a validator
    attributes::inner_attributes(p);

    while !p.at(EOF) && !p.at(T!['}']) {
        // test nocontentexpr
        // fn foo(){
        //     ;;;some_expr();;;;{;;;};;;;Ok(())
        // }

        // test nocontentexpr_after_item
        // fn simple_function() {
        //     enum LocalEnum {
        //         One,
        //         Two,
        //     };
        //     fn f() {};
        //     struct S {};
        // }

        if p.at(T![;]) {
            p.bump(T![;]);
            continue;
        }

        stmt(p, StmtWithSemi::Yes)
    }
}

#[derive(Clone, Copy)]
struct Restrictions {
    forbid_structs: bool,
    prefer_stmt: bool,
}

/// Binding powers of operators for a Pratt parser.
///
/// See https://www.oilshell.org/blog/2016/11/03.html
#[rustfmt::skip]
fn current_op(p: &Parser) -> (u8, SyntaxKind) {
    const NOT_AN_OP: (u8, SyntaxKind) = (0, T![@]);
    match p.current() {
        T![|] if p.at(T![||])  => (3,  T![||]),
        T![|] if p.at(T![|=])  => (1,  T![|=]),
        T![|]                  => (6,  T![|]),
        T![>] if p.at(T![>>=]) => (1,  T![>>=]),
        T![>] if p.at(T![>>])  => (9,  T![>>]),
        T![>] if p.at(T![>=])  => (5,  T![>=]),
        T![>]                  => (5,  T![>]),
        T![=] if p.at(T![=>])  => NOT_AN_OP,
        T![=] if p.at(T![==])  => (5,  T![==]),
        T![=]                  => (1,  T![=]),
        T![<] if p.at(T![<=])  => (5,  T![<=]),
        T![<] if p.at(T![<<=]) => (1,  T![<<=]),
        T![<] if p.at(T![<<])  => (9,  T![<<]),
        T![<]                  => (5,  T![<]),
        T![+] if p.at(T![+=])  => (1,  T![+=]),
        T![+]                  => (10, T![+]),
        T![^] if p.at(T![^=])  => (1,  T![^=]),
        T![^]                  => (7,  T![^]),
        T![%] if p.at(T![%=])  => (1,  T![%=]),
        T![%]                  => (11, T![%]),
        T![&] if p.at(T![&=])  => (1,  T![&=]),
        T![&] if p.at(T![&&])  => (4,  T![&&]),
        T![&]                  => (8,  T![&]),
        T![/] if p.at(T![/=])  => (1,  T![/=]),
        T![/]                  => (11, T![/]),
        T![*] if p.at(T![*=])  => (1,  T![*=]),
        T![*]                  => (11, T![*]),
        T![.] if p.at(T![..=]) => (2,  T![..=]),
        T![.] if p.at(T![..])  => (2,  T![..]),
        T![!] if p.at(T![!=])  => (5,  T![!=]),
        T![-] if p.at(T![-=])  => (1,  T![-=]),
        T![-]                  => (10, T![-]),

        _                      => NOT_AN_OP
    }
}

// Parses expression with binding power of at least bp.
fn expr_bp(p: &mut Parser, r: Restrictions, bp: u8) -> (Option<CompletedMarker>, BlockLike) {
    let mut lhs = match lhs(p, r) {
        Some((lhs, blocklike)) => {
            // test stmt_bin_expr_ambiguity
            // fn foo() {
            //     let _ = {1} & 2;
            //     {1} &2;
            // }
            if r.prefer_stmt && blocklike.is_block() {
                return (Some(lhs), BlockLike::Block);
            }
            lhs
        }
        None => return (None, BlockLike::NotBlock),
    };

    loop {
        let is_range = p.at(T![..]) || p.at(T![..=]);
        let (op_bp, op) = current_op(p);
        if op_bp < bp {
            break;
        }
        let m = lhs.precede(p);
        p.bump(op);

        expr_bp(p, r, op_bp + 1);
        lhs = m.complete(p, if is_range { RANGE_EXPR } else { BIN_EXPR });
    }
    (Some(lhs), BlockLike::NotBlock)
}

const LHS_FIRST: TokenSet = atom::ATOM_EXPR_FIRST.union(token_set![AMP, STAR, EXCL, DOT, MINUS]);

fn lhs(p: &mut Parser, r: Restrictions) -> Option<(CompletedMarker, BlockLike)> {
    let m;
    let kind = match p.current() {
        // test ref_expr
        // fn foo() {
        //     let _ = &1;
        //     let _ = &mut &f();
        // }
        T![&] => {
            m = p.start();
            p.bump(T![&]);
            p.eat(T![mut]);
            REF_EXPR
        }
        // test unary_expr
        // fn foo() {
        //     **&1;
        //     !!true;
        //     --1;
        // }
        T![*] | T![!] | T![-] => {
            m = p.start();
            p.bump_any();
            PREFIX_EXPR
        }
        _ => {
            // test full_range_expr
            // fn foo() { xs[..]; }
            for &op in [T![..=], T![..]].iter() {
                if p.at(op) {
                    m = p.start();
                    p.bump(op);
                    if p.at_ts(EXPR_FIRST) {
                        expr_bp(p, r, 2);
                    }
                    return Some((m.complete(p, RANGE_EXPR), BlockLike::NotBlock));
                }
            }

            // test expression_after_block
            // fn foo() {
            //    let mut p = F{x: 5};
            //    {p}.x = 10;
            // }
            //
            let (lhs, blocklike) = atom::atom_expr(p, r)?;
            return Some(postfix_expr(
                p,
                lhs,
                blocklike,
                !(r.prefer_stmt && blocklike.is_block()),
                r.forbid_structs,
            ));
        }
    };
    expr_bp(p, r, 255);
    Some((m.complete(p, kind), BlockLike::NotBlock))
}

fn postfix_expr(
    p: &mut Parser,
    mut lhs: CompletedMarker,
    // Calls are disallowed if the type is a block and we prefer statements because the call cannot be disambiguated from a tuple
    // E.g. `while true {break}();` is parsed as
    // `while true {break}; ();`
    mut block_like: BlockLike,
    mut allow_calls: bool,
    forbid_structs: bool,
) -> (CompletedMarker, BlockLike) {
    loop {
        lhs = match p.current() {
            // test stmt_postfix_expr_ambiguity
            // fn foo() {
            //     match () {
            //         _ => {}
            //         () => {}
            //         [] => {}
            //     }
            // }
            T!['('] if allow_calls => call_expr(p, lhs),
            T!['['] if allow_calls => index_expr(p, lhs),
            T![.] => match postfix_dot_expr(p, lhs, forbid_structs) {
                Ok(it) => it,
                Err(it) => {
                    lhs = it;
                    break;
                }
            },
            T![?] => try_expr(p, lhs),
            T![as] => cast_expr(p, lhs),
            _ => break,
        };
        allow_calls = true;
        block_like = BlockLike::NotBlock;
    }
    return (lhs, block_like);

    fn postfix_dot_expr(
        p: &mut Parser,
        lhs: CompletedMarker,
        forbid_structs: bool,
    ) -> Result<CompletedMarker, CompletedMarker> {
        assert!(p.at(T![.]));
        if p.nth(1) == IDENT && (p.nth(2) == T!['('] || p.nth_at(2, T![::])) {
            return Ok(method_call_expr(p, lhs));
        }

        // test await_expr
        // fn foo() {
        //     x.await;
        //     x.0.await;
        //     x.0().await?.hello();
        // }
        if p.nth(1) == T![await] {
            let m = lhs.precede(p);
            p.bump(T![.]);
            p.bump(T![await]);
            return Ok(m.complete(p, AWAIT_EXPR));
        }

        // test postfix_range
        // fn foo() {
        //     let x = 1..;
        //     match 1.. { _ => () };
        //     match a.b()..S { _ => () };
        // }
        for &(op, la) in &[(T![..=], 3), (T![..], 2)] {
            if p.at(op) {
                let next_token = p.nth(la);
                let has_trailing_expression =
                    !(forbid_structs && next_token == T!['{']) && EXPR_FIRST.contains(next_token);
                return if has_trailing_expression {
                    Err(lhs)
                } else {
                    let m = lhs.precede(p);
                    p.bump(op);
                    Ok(m.complete(p, RANGE_EXPR))
                };
            }
        }

        Ok(field_expr(p, lhs))
    }
}

// test call_expr
// fn foo() {
//     let _ = f();
//     let _ = f()(1)(1, 2,);
//     let _ = f(<Foo>::func());
//     f(<Foo as Trait>::func());
// }
fn call_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T!['(']));
    let m = lhs.precede(p);
    arg_list(p);
    m.complete(p, CALL_EXPR)
}

// test index_expr
// fn foo() {
//     x[1][2];
// }
fn index_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T!['[']));
    let m = lhs.precede(p);
    p.bump(T!['[']);
    expr(p);
    p.expect(T![']']);
    m.complete(p, INDEX_EXPR)
}

// test method_call_expr
// fn foo() {
//     x.foo();
//     y.bar::<T>(1, 2,);
// }
fn method_call_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![.]) && p.nth(1) == IDENT && (p.nth(2) == T!['('] || p.nth_at(2, T![::])));
    let m = lhs.precede(p);
    p.bump_any();
    name_ref(p);
    type_args::opt_type_arg_list(p, true);
    if p.at(T!['(']) {
        arg_list(p);
    }
    m.complete(p, METHOD_CALL_EXPR)
}

// test field_expr
// fn foo() {
//     x.foo;
//     x.0.bar;
//     x.0();
// }

// test_err bad_tuple_index_expr
// fn foo() {
//     x.0.;
//     x.1i32;
//     x.0x01;
// }
#[allow(clippy::if_same_then_else)]
fn field_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![.]));
    let m = lhs.precede(p);
    p.bump(T![.]);
    if p.at(IDENT) || p.at(INT_NUMBER) {
        name_ref_or_index(p)
    } else if p.at(FLOAT_NUMBER) {
        // FIXME: How to recover and instead parse INT + T![.]?
        p.bump_any();
    } else {
        p.error("expected field name or number")
    }
    m.complete(p, FIELD_EXPR)
}

// test try_expr
// fn foo() {
//     x?;
// }
fn try_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![?]));
    let m = lhs.precede(p);
    p.bump(T![?]);
    m.complete(p, TRY_EXPR)
}

// test cast_expr
// fn foo() {
//     82 as i32;
//     81 as i8 + 1;
//     79 as i16 - 1;
//     0x36 as u8 <= 0x37;
// }
fn cast_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![as]));
    let m = lhs.precede(p);
    p.bump(T![as]);
    // Use type_no_bounds(), because cast expressions are not
    // allowed to have bounds.
    types::type_no_bounds(p);
    m.complete(p, CAST_EXPR)
}

fn arg_list(p: &mut Parser) {
    assert!(p.at(T!['(']));
    let m = p.start();
    p.bump(T!['(']);
    while !p.at(T![')']) && !p.at(EOF) {
        if !p.at_ts(EXPR_FIRST) {
            p.error("expected expression");
            break;
        }
        expr(p);
        if !p.at(T![')']) && !p.expect(T![,]) {
            break;
        }
    }
    p.eat(T![')']);
    m.complete(p, ARG_LIST);
}

// test path_expr
// fn foo() {
//     let _ = a;
//     let _ = a::b;
//     let _ = ::a::<b>;
//     let _ = format!();
// }
fn path_expr(p: &mut Parser, r: Restrictions) -> (CompletedMarker, BlockLike) {
    assert!(paths::is_path_start(p));
    let m = p.start();
    paths::expr_path(p);
    match p.current() {
        T!['{'] if !r.forbid_structs => {
            record_field_list(p);
            (m.complete(p, RECORD_LIT), BlockLike::NotBlock)
        }
        T![!] if !p.at(T![!=]) => {
            let block_like = items::macro_call_after_excl(p);
            (m.complete(p, MACRO_CALL), block_like)
        }
        _ => (m.complete(p, PATH_EXPR), BlockLike::NotBlock),
    }
}

// test record_lit
// fn foo() {
//     S {};
//     S { x, y: 32, };
//     S { x, y: 32, ..Default::default() };
//     TupleStruct { 0: 1 };
// }
pub(crate) fn record_field_list(p: &mut Parser) {
    assert!(p.at(T!['{']));
    let m = p.start();
    p.bump(T!['{']);
    while !p.at(EOF) && !p.at(T!['}']) {
        match p.current() {
            // test record_literal_field_with_attr
            // fn main() {
            //     S { #[cfg(test)] field: 1 }
            // }
            IDENT | INT_NUMBER | T![#] => {
                let m = p.start();
                attributes::outer_attributes(p);
                name_ref_or_index(p);
                if p.eat(T![:]) {
                    expr(p);
                }
                m.complete(p, RECORD_FIELD);
            }
            T![.] if p.at(T![..]) => {
                p.bump(T![..]);
                expr(p);
            }
            T!['{'] => error_block(p, "expected a field"),
            _ => p.err_and_bump("expected identifier"),
        }
        if !p.at(T!['}']) {
            p.expect(T![,]);
        }
    }
    p.expect(T!['}']);
    m.complete(p, RECORD_FIELD_LIST);
}
