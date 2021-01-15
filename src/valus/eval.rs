use core::ptr::NonNull;

use crate::valus::{
  dag::{
    alloc_lit,
    clear_copies,
    free_dead_node,
    new_app,
    new_lambda,
    replace_child,
    App,
    Lam,
    ParentCell,
    Var,
    DAG,
  },
  dll::*,
  primop::apply_bin_op,
};

// The core up-copy function.
pub fn upcopy(new_child: DAG, cc: ParentCell) {
  unsafe {
    match cc {
      ParentCell::LamBod(parent) => {
        let Lam { var, parents: grandparents, .. } = *parent.as_ptr();
        let new_lam = new_lambda(var, new_child);
        for grandparent in DLL::iter_option(grandparents) {
          upcopy(DAG::Lam(new_lam), *grandparent)
        }
      }
      ParentCell::AppFun(parent) => {
        let App { copy, arg, parents: grandparents, .. } = *parent.as_ptr();
        match copy {
          Some(cache) => {
            (*cache.as_ptr()).func = new_child;
          }
          None => {
            let new_app = new_app(new_child, arg);
            (*parent.as_ptr()).copy = Some(new_app);
            for grandparent in DLL::iter_option(grandparents) {
              upcopy(DAG::App(new_app), *grandparent)
            }
          }
        }
      }
      ParentCell::AppArg(parent) => {
        let App { copy, func, parents: grandparents, .. } = *parent.as_ptr();
        match copy {
          Some(cache) => {
            (*cache.as_ptr()).arg = new_child;
          }
          None => {
            let new_app = new_app(func, new_child);
            (*parent.as_ptr()).copy = Some(new_app);
            for grandparent in DLL::iter_option(grandparents) {
              upcopy(DAG::App(new_app), *grandparent)
            }
          }
        }
      }
      ParentCell::Root => (),
    }
  }
}

// Contract a lambda redex, return the body.
pub fn reduce_lam(redex: NonNull<App>, lam: NonNull<Lam>) -> DAG {
  unsafe {
    let App { arg, .. } = *redex.as_ptr();
    let Lam { var, body, parents: lam_parents, .. } = *lam.as_ptr();
    let Var { parents: var_parents, .. } = *var.as_ptr();
    let ans = if DLL::is_singleton(lam_parents) {
      replace_child(DAG::Var(var), arg);
      // We have to read `body` again because `lam`'s body could be mutated
      // through `replace_child`
      (*lam.as_ptr()).body
    }
    else if var_parents.is_none() {
      body
    }
    else {
      let mut input = body;
      let mut topapp = None;
      let mut result = arg;
      let mut vars = vec![];
      loop {
        match input {
          DAG::Lam(lam) => {
            let Lam { body, var, .. } = *lam.as_ptr();
            input = body;
            vars.push(var);
          }
          DAG::App(app) => {
            let App { arg: top_arg, func, .. } = *app.as_ptr();
            let new_app = new_app(func, top_arg);
            (*app.as_ptr()).copy = Some(new_app);
            topapp = Some(app);
            for parent in DLL::iter_option(var_parents) {
              upcopy(arg, *parent);
            }
            result = DAG::App(new_app);
            break;
          }
          // Otherwise it must be `var`, since `var` necessarily appears inside
          // `body`
          _ => break,
        }
      }
      while let Some(var) = vars.pop() {
        result = DAG::Lam(new_lambda(var, result));
      }
      topapp.map_or((), |app| clear_copies(lam.as_ref(), &mut *app.as_ptr()));
      result
    };
    replace_child(DAG::App(redex), ans);
    free_dead_node(DAG::App(redex));
    ans
  }
}
// Reduce term to its weak head normal form
pub fn whnf(mut node: DAG) -> DAG {
  let mut trail = vec![];
  loop {
    match node {
      DAG::App(link) => unsafe {
        trail.push(link);
        node = (*link.as_ptr()).func;
      },
      DAG::Lam(lam_link) => {
        if let Some(app_link) = trail.pop() {
          node = reduce_lam(app_link, lam_link);
        }
        else {
          break;
        }
      }
      DAG::Opr(link) => unsafe {
        let len = trail.len();
        if len >= 2 {
          let arg1 = whnf((*trail[len - 2].as_ptr()).arg);
          let arg2 = whnf((*trail[len - 1].as_ptr()).arg);
          match (arg1, arg2) {
            (DAG::Lit(x), DAG::Lit(y)) => {
              let opr = (*link.as_ptr()).opr;
              let x = (*x.as_ptr()).val.clone();
              let y = (*y.as_ptr()).val.clone();
              let res = apply_bin_op(opr, y, x);
              if let Some(res) = res {
                trail.pop();
                trail.pop();
                node = DAG::Lit(alloc_lit(res));
                replace_child(arg1, node);
                free_dead_node(arg1);
              }
              // TODO: (#cst (Nat 256) 0d1)
              //(DAG::App(x), DAG::Lit(y)) => {
              //
              //}
              else {
                break;
              }
            }
            _ => break,
          }
        }
        break;
      },
      _ => break,
    }
  }
  if trail.is_empty() {
    return node;
  }
  DAG::App(trail[0])
}

// Reduce term to its normal form
pub fn norm(mut top_node: DAG) -> DAG {
  top_node = whnf(top_node);
  let mut trail = vec![top_node];
  while let Some(node) = trail.pop() {
    match node {
      DAG::App(link) => unsafe {
        let app = &mut *link.as_ptr();
        trail.push(whnf(app.func));
        trail.push(whnf(app.arg));
      },
      DAG::Lam(link) => unsafe {
        let lam = &mut *link.as_ptr();
        trail.push(whnf(lam.body));
      },
      _ => (),
    }
  }
  top_node
}

use hashexpr::span::Span;

pub fn parse(
  i: &str,
) -> nom::IResult<Span, DAG, crate::parse::error::ParseError<Span>> {
  let (i, tree) = crate::parse::term::parse(i)?;
  let (i, _) = nom::character::complete::multispace0(i)?;
  let (i, _) = nom::combinator::eof(i)?;
  let dag = DAG::from_term(tree);
  Ok((i, dag))
}

#[cfg(test)]
mod test {
  use super::{
    norm,
    parse,
  };

  #[test]
  pub fn parser() {
    fn parse_assert(input: &str) {
      match parse(&input) {
        Ok((_, dag)) => assert_eq!(format!("{}", dag), input),
        Err(_) => panic!("Did not parse."),
      }
    }
    parse_assert("λ x => x");
    parse_assert("λ x y => x y");
    parse_assert("λ y => (λ x => x) y");
    parse_assert("λ y => (λ z => z z) ((λ x => x) y)");
  }

  #[test]
  pub fn reducer() {
    fn norm_assert(input: &str, result: &str) {
      match parse(&input) {
        Ok((_, dag)) => assert_eq!(format!("{}", norm(dag)), result),
        Err(_) => panic!("Did not parse."),
      }
    }
    // Already normalized
    norm_assert("λ x => x", "λ x => x");
    norm_assert("λ x y => x y", "λ x y => x y");
    // Not normalized cases
    norm_assert("λ y => (λ x => x) y", "λ y => y");
    norm_assert("λ y => (λ z => z z) ((λ x => x) y)", "λ y => y y");
    // // Church arithmetic
    let zero = "λ s z => z";
    let three = "λ s z => s (s (s z))";
    let four = "λ s z => s (s (s (s z)))";
    let seven = "λ s z => s (s (s (s (s (s (s z))))))";
    let add = "λ m n s z => m s (n s z)";
    let is_three = format!("(({}) ({}) {})", add, zero, three);
    let is_seven = format!("(({}) ({}) {})", add, four, three);
    norm_assert(&is_three, three);
    norm_assert(&is_seven, seven);
    let id = "λ x => x";
    norm_assert(
      &format!("({three}) (({three}) ({id})) ({id})", id = id, three = three),
      id,
    );
  }
}
