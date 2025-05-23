
Require Import ZArith.
From QuickChick Require Import QuickChick.
From ExtLib Require Import Monad.
From ExtLib.Data.Monads Require Import OptionMonad.
Import QcNotation.
Import MonadNotation.
From Rocq Require Import List.
Import ListNotations.


Local Open Scope Z_scope.

Notation "A <?? B" := (Z_lt_le_dec A B) (at level 70, no associativity).


Inductive Color := R | B.
Derive (Arbitrary, Show) for Color.


Inductive Tree :=
    | E : Tree
    | T : Color -> Tree -> Z -> Z -> Tree -> Tree.

Derive (Show) for Tree.

Axiom fuel : nat. Extract Constant fuel => "100000".

(* ---------- *)

(* -- Used for insert and delete. *)

Definition blacken (t: Tree) : Tree :=
    match t with
    | E => E
    | (T _ a x vx b) => T B a x vx b
    end.


Definition redden (t: Tree) : option Tree :=
    match t with
    | T B a x vx b => Some(T R a x vx b)
    | _ => None
    end.

Definition balance (col: Color) (tl: Tree) (key: Z) (val: Z) (tr: Tree) : Tree :=
    match col, tl, key, val, tr with