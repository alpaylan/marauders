use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Color {
    R,
    B,
}

impl Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Color::R => write!(f, "(R)"),
            Color::B => write!(f, "(B)"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Tree {
    E,
    T(Color, Box<Tree>, i32, i32, Box<Tree>),
}

impl Display for Tree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tree::E => write!(f, "(E)"),
            Tree::T(color, left, key, value, right) => {
                write!(
                    f,
                    "(T {} {} {} {} {})",
                    color,
                    left,
                    key,
                    value,
                    right
                )
            }
        }
    }
}

use Color::*;
use Tree::*;

const FUEL: usize = 10000;

pub(crate) fn elems(t: &Tree) -> Vec<(i32, i32)> {
    fn go(t: &Tree, acc: &mut Vec<(i32, i32)>) {
        match t {
            E => {}
            T(_, box a, x, vx, box b) => {
                go(a, acc);
                acc.push((*x, *vx));
                go(b, acc);
            }
        }
    }
    let mut kvs = Vec::new();
    go(t, &mut kvs);
    kvs
}

pub(crate) fn blacken(t: Tree) -> Tree {
    match t {
        E => E,
        T(_, a, x, vx, b) => T(B, a, x, vx, b),
    }
}

pub(crate) fn redden(t: Tree) -> Option<Tree> {
    match t {
        T(B, a, x, vx, b) => Some(T(R, a, x, vx, b)),
        _ => None,
    }
}

pub(crate) fn balance(col: Color, tl: Tree, key: i32, val: i32, tr: Tree) -> Tree {
    match (col, tl, key, val, tr) {
        /*| */
        (B, T(R, box T(R, a, x, vx, b), y, vy, c), z, vz, d) => T(
            R,
            Box::new(T(B, a, x, vx, b)),
            y,
            vy,
            Box::new(T(B, c, z, vz, Box::new(d))),
        ),
        /*|| swap_cd */
        /*|
        (B, T(R, box T(R, a, x, vx, b), y, vy, c), z, vz, d) => T(
            R,
            Box::new(T(B, a, x, vx, b)),
            y,
            vy,
            Box::new(T(B, Box::new(d), z, vz, c)),
        ),
        */
        /* |*/
        (B, T(R, a, x, vx, box T(R, b, y, vy, c)), z, vz, d) => T(
            R,
            Box::new(T(B, a, x, vx, b)),
            y,
            vy,
            Box::new(T(B, c, z, vz, Box::new(d))),
        ),
        /*| */
        (B, a, x, vx, T(R, box T(R, b, y, vy, c), z, vz, d)) => T(
            R,
            Box::new(T(B, Box::new(a), x, vx, b)),
            y,
            vy,
            Box::new(T(B, c, z, vz, d)),
        ),
        /*|| swap_bc */
        /*|
        (B, a, x, vx, T(R, box T(R, b, y, vy, c), z, vz, d)) => T(
            R,
            Box::new(T(B, Box::new(a), x, vx, c)),
            y,
            vy,
            Box::new(T(B, b, z, vz, d)),
        ),
        */
        /* |*/
        (B, a, x, vx, T(R, b, y, vy, box T(R, c, z, vz, d))) => T(
            R,
            Box::new(T(B, Box::new(a), x, vx, b)),
            y,
            vy,
            Box::new(T(B, c, z, vz, d)),
        ),
        (rb, a, x, vx, b) => T(rb, Box::new(a), x, vx, Box::new(b)),
    }
}

pub(crate) fn insert(key: i32, val: i32, t: Tree) -> Tree {
    fn ins(x: i32, vx: i32, s: Tree) -> Tree {
        match (x, vx, s) {
            (x, vx, E) => {
                /*| */
                T(R, Box::new(E), x, vx, Box::new(E))
                /*|| miscolor_insert */
                /*|
                T(B, Box::new(E), x, vx, Box::new(E))
                */
                /* |*/
            }
            (x, vx, T(rb, box a, y, vy, box b)) => {
                /*| */
                                                if x < y {
                                                    balance(rb, ins(x, vx, a), y, vy, b)
                                                } else if y < x {
                                                    balance(rb, a, y, vy, ins(x, vx, b))
                                                } else {
                                                    T(rb, Box::new(a), y, vx, Box::new(b))
                                                }
                /*|| insert_1 */
                /*|
                T(R, Box::new(E), x, vx, Box::new(E))
                */
                /*|| insert_2 */
                /*|
                if x < y {
                    balance(rb, ins(x, vx, a), y, vy, b)
                } else {
                    T(rb, Box::new(a), y, vx, Box::new(b))
                }
                */
                /*|| insert_3 */
                /*|
                if x < y {
                    balance(rb, ins(x, vx, a), y, vy, b)
                } else if y < x {
                    balance(rb, a, y, vy, ins(x, vx, b))
                } else {
                    T(rb, Box::new(a), y, vy, Box::new(b))
                }
                */
                /*|| no_balance_insert_1 */
                /*|
                if x < y {
                    T(rb, Box::new(ins(x, vx, a)), y, vy, Box::new(b))
                } else if y < x {
                    balance(rb, a, y, vy, ins(x, vx, b))
                } else {
                    T(rb, Box::new(a), y, vx, Box::new(b))
                }
                */
                /*|| no_balance_insert_2 */
                /*|
                if x < y {
                    balance(rb, ins(x, vx, a), y, vy, b)
                } else if y < x {
                    T(rb, Box::new(a), y, vy, Box::new(insert(x, vx, b)))
                } else {
                    T(rb, Box::new(a), y, vx, Box::new(b))
                }
                */
                /* |*/
            }
        }
    }
    blacken(ins(key, val, t))
}

pub(crate) fn bal_left(tl: Tree, k: i32, v: i32, tr: Tree) -> Option<Tree> {
    match (tl, k, v, tr) {
        (T(R, a, x, vx, b), y, vy, c) => {
            Some(T(R, Box::new(T(B, a, x, vx, b)), y, vy, Box::new(c)))
        }
        (bl, x, vx, T(B, a, y, vy, b)) => Some(balance(B, bl, x, vx, T(R, a, y, vy, b))),
        (bl, x, vx, T(R, box T(B, a, y, vy, box b), z, vz, box c)) => {
            /*| */
            let cp = redden(c)?;
            Some(T(
                R,
                Box::new(T(B, Box::new(bl), x, vx, a)),
                y,
                vy,
                Box::new(balance(B, b, z, vz, cp)),
            ))
            /*|| miscolor_balLeft */
            /*|
            Some(T(
                R,
                Box::new(T(B, Box::new(bl), x, vx, a)),
                y,
                vy,
                Box::new(balance(B, b, z, vz, c)),
            ))
            */
            /* |*/
        }
        (_, _, _, _) => None,
    }
}

pub(crate) fn bal_right(tl: Tree, k: i32, v: i32, tr: Tree) -> Option<Tree> {
    match (tl, k, v, tr) {
        (a, x, vx, T(R, b, y, vy, c)) => {
            Some(T(R, Box::new(a), x, vx, Box::new(T(B, b, y, vy, c))))
        }
        (T(B, a, x, vx, b), y, vy, bl) => Some(balance(B, T(R, a, x, vx, b), y, vy, bl)),
        (T(R, box a, x, vx, box T(B, box b, y, vy, c)), z, vz, bl) => {
            /*| */
            let ap = redden(a)?;
            Some(T(
                R,
                Box::new(balance(B, ap, x, vx, b)),
                y,
                vy,
                Box::new(T(B, c, z, vz, Box::new(bl))),
            ))
            /*|| miscolor_balRight */
            /*|
            Some(T(
                R,
                Box::new(balance(B, a, x, vx, b)),
                y,
                vy,
                Box::new(T(B, c, z, vz, Box::new(bl))),
            ))
            */
            /* |*/
        }
        (_, _, _, _) => None,
    }
}

pub(crate) fn _join(t1: Tree, t2: Tree, f: usize) -> Option<Tree> {
    if f == 0 {
        return None;
    }
    let fp = f - 1;
    match (t1, t2) {
        (E, a) => Some(a),
        (a, E) => Some(a),
        (T(R, a, x, vx, box b), T(R, box c, y, vy, d)) => match _join(b, c, fp) {
            None => None,
            Some(T(R, bp, z, vz, cp)) => {
                /*| */
                Some(T(
                    R,
                    Box::new(T(R, a, x, vx, bp)),
                    z,
                    vz,
                    Box::new(T(R, cp, y, vy, d)),
                ))
                /*|| miscolor_join_1 */
                /*|
                Some(T(
                    R,
                    Box::new(T(B, a, x, vx, bp)),
                    z,
                    vz,
                    Box::new(T(B, cp, y, vy, d)),
                ))
                */
                /* |*/
            }
            Some(bc) => Some(T(R, a, x, vx, Box::new(T(R, Box::new(bc), y, vy, d)))),
        },
        (T(B, a, x, vx, box b), T(B, box c, y, vy, d)) => match _join(b, c, fp) {
            None => None,
            Some(T(R, bp, z, vz, cp)) => {
                /*| */
                Some(T(
                    R,
                    Box::new(T(B, a, x, vx, bp)),
                    z,
                    vz,
                    Box::new(T(B, cp, y, vy, d)),
                ))
                /*|| miscolor_join_2 */
                /*|
                Some(T(
                    R,
                    Box::new(T(R, a, x, vx, bp)),
                    z,
                    vz,
                    Box::new(T(R, cp, y, vy, d)),
                ))
                */
                /* |*/
            }
            Some(bc) => bal_left(*a, x, vx, T(B, Box::new(bc), y, vy, d)),
        },
        (a, T(R, box b, x, vx, c)) => match _join(a, b, fp) {
            None => None,
            Some(tp) => Some(T(R, Box::new(tp), x, vx, c)),
        },
        (T(R, a, x, vx, box b), c) => {
            let tp = _join(b, c, fp)?;
            Some(T(R, a, x, vx, Box::new(tp)))
        }
    }
}

pub(crate) fn join(t1: Tree, t2: Tree) -> Option<Tree> {
    _join(t1, t2, FUEL)
}

pub(crate) fn del(x: i32, s: Tree, f: usize) -> Option<Tree> {
    if f == 0 {
        return None;
    }
    let fp = f - 1;

    match s {
        E => Some(E),
        T(_, box a, y, vy, box b) => {
            /*| */
            if x < y {
                let tp = del_left(x, a, y, vy, b, fp)?;
                Some(tp)
            } else if y < x {
                let tp = del_right(x, a, y, vy, b, fp)?;
                Some(tp)
            } else {
                let tp = join(a, b)?;
                Some(tp)
            }
            /*|| delete_4 */
            /*|
            if x < y {
                del(x, a, fp)
            } else if y < x {
                del(x, b, fp)
            } else {
                join(a, b)
            }
            */
            /*|| delete_5 */
            /*|
            if y < x {
                del_left(x, a, y, vy, b, fp)
            } else if x < y {
                del_right(x, a, y, vy, b, fp)
            } else {
                join(a, b)
            }
            */
            /* |*/
        }
    }
}

fn del_left(x: i32, dl: Tree, dy: i32, dvy: i32, dr: Tree, f: usize) -> Option<Tree> {
    if f == 0 {
        return None;
    }
    let fp = f - 1;

    match (dl, dy, dvy, dr) {
        (T(B, al, ax, avx, ar), y, vy, b) => {
            let tp = del(x, T(B, al, ax, avx, ar), fp)?;
            let tpp = bal_left(tp, y, vy, b)?;
            Some(tpp)
        }
        (a, y, vy, b) => {
            let tp = del(x, a, fp)?;
            Some(T(R, Box::new(tp), y, vy, Box::new(b)))
        }
    }
}

fn del_right(x: i32, dl: Tree, dy: i32, dvy: i32, dr: Tree, f: usize) -> Option<Tree> {
    if f == 0 {
        return None;
    }
    let fp = f - 1;

    match (dl, dy, dvy, dr) {
        (a, y, vy, T(B, bl, bx, bvx, br)) => {
            let tp = del(x, T(B, bl, bx, bvx, br), fp)?;
            let tpp = bal_right(a, y, vy, tp)?;
            Some(tpp)
        }
        (a, y, vy, b) => {
            let tp = del(x, b, fp)?;
            Some(T(R, Box::new(a), y, vy, Box::new(tp)))
        }
    }
}

pub(crate) fn delete(x: i32, t: Tree) -> Option<Tree> {
    /*| */
    let tp = del(x, t, FUEL)?;
    Some(blacken(tp))
    /*|| miscolor_delete */
    /*|
    del (x, t, FUEL)
    */
    /* |*/
}

pub(crate) fn find(x: i32, t: Tree) -> Option<i32> {
    match t {
        E => None,
        T(_, box l, y, vy, box r) => {
            if x < y {
                find(x, l)
            } else if y < x {
                find(x, r)
            } else {
                Some(vy)
            }
        }
    }
}

pub(crate) fn size(t: Tree) -> usize {
    match t {
        E => 0,
        T(_, box l, _, _, box r) => 1 + size(l) + size(r),
    }
}
