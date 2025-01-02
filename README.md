# Marauder

Marauder is a command line tool built for inline mutation testing. It is designed to be used in conjunction with a test suite to identify and apply mutations to code.

Current Capabilities:

- [x] Report the set of variants in a file.
- [x] Activate a variant in a file.
- [x] Deactivate a variant in a file.
- [x] Reset a file to its original state.
- [ ] Run marauder on a directory.
- [ ] Support mutation expressions[mutation-expressions]
- [ ] Run marauder on incremental mode[incremental-mode]
- [ ] Run marauder on copy mode[copy-mode]
- [ ] Support C Preprocessor Macros[cpp-macros]
- [ ] Support Functional Mutations[functional-mutations]
- [ ] Support Git Patch Mutations[git-patch-mutations]

[mutation-expressions]: #mutation-expressions
[functional-mutations]: #functional-mutations

## Installation

```bash
cargo install marauder
```

## Mutation Expressions

Mutation expressions are a small language for expressing a sequence of mutations to apply to a file. The structure of the language is as follows:

```bnf
expr =  expr + expr
        | expr * expr
        | +tag
        | *tag
        | varint
        | variation
```

Using the unary and binary operations (+) and (*), users can express applying mutations
at the same time(*), or applying mutations sequentially(+). The evaluation strategy is
to turn the expression into sum of products form, e.g `(a + b) * (c + d) = ac + ad + bc + bd`.

[copy-mode]: .
[incremental-mode]: .

The resulting expression is then read as a list, `[ac, ad, bc, bd]`, where each element is a
set of mutations to apply to the file. There are 2 mechanisms for the successive mutation
application, one is `incremental mode` that takes an index of the last applied mutation,
and `copy mode` that creates a copy for each successive set of mutations and returns
the user all the copies.

## Mutation Syntaxes

marauder supports multiple mechanisms for expressing mutations within code, the default
mode is the `comment syntax`, in which users can express mutations by adding comments
to the code. The comment syntax is as follows:

```rust
fn add(a: i32, b: i32) -> i32 {
    /*! add_variation */
    a + b
    /*!! add_mutation_1 */
    /*!
    a - b
    */
    /*! add_mutation_2 */
    /*!
    a * b
    */
    /* !*/
}
```

This code has 1 variation, named `add_variation`, and 2 variants within the variation, named `add_mutation_1` and `add_mutation_2`. A Pest grammar of the syntax can be found at `src/comment.pest`. It is also possible to tag variations and variants with tags, as tags can be used to select specific subsets of mutations to apply.

### Functional Mutations