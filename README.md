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

## Usage

```bash
$ marauder --help
> 
Usage: marauder <COMMAND>
    Commands:
    list   List variations in the code
    set    Set active variant
    unset  Unset active variant
    reset  Reset all variationts to base
    help   Print this message or the help of the given subcommand(s)

    Options:
    -h, --help  Print help
```

Users can list the variations in a file or directory using the `list` command:

```bash
$ marauder list --path <path-to-file>
> 
    test/BST.v:21 (name: insert, active: base, variants: ["insert_1", "insert_2", "insert_3"], tags: ["new", "easy"])
    test/BST.v:57 (name: anonymous, active: base, variants: ["delete_4", "delete_5"], tags: [])
    test/BST.v:104 (name: anonymous, active: base, variants: ["union_6", "union_7", "union_8"], tags: [])
```

Users can set the active variant in a file or directory using the `set` command:

```bash
$ marauder set --path <path-to-file> --variant <variant-name>
> active variant set to 'insert_1' in 'test/BST.v:21'
```

Users can unset the active variant in a file or directory using the `unset` command:

```bash
$ marauder unset --path <path-to-file>
> active variant unset to base in 'test/BST.v:21'
```

Users can reset all variations in a file or directory using the `reset` command:

```bash
$ marauder reset --path <path-to-file>
> all variations reset to base in 'test/BST.v'
```

## Mutation Expressions

> [!NOTE]
> This feature is not yet implemented.

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

### Preprocessor Macros

> [!NOTE]
> This feature is not yet implemented.

C preprocessor macros are a language independent way to express mutations in code. The syntax is as follows:

```c
int add(int a, int b) {
    #if defined(add_variation) && !defined(add_mutation_1) && !defined(add_mutation_2)
    return a + b;
    #elif defined(add_mutation_1)
    return a - b;
    #elif defined(add_mutation_2)
    return a * b;
    #endif
}
```

### Functional Mutations

> [!NOTE]
> This feature is not yet implemented.

Functional mutations are a mechanism for expressing mutations within code, using environment variables. The syntax is as follows:

```rust
fn add(a: i32, b: i32) -> i32 {
    match std::env::var("add_variation") {
        Ok("base") | Err(_) => a + b,
        Ok("add_mutation_1") => a - b,
        Ok("add_mutation_2") => a * b,
        _ => panic!("Unknown variation"),
    }
}
```

The environment variable `add_variation` is used to select the variation to apply. A very
important benefit of this mechanism is that it does not require multiple compilation steps,
which is an issue with all other mutation types. Although, the downside is it is very intrusive within the code, reducing readability, and maintainability.

### Mutation Conversion

> [!NOTE]
> This feature is not yet implemented.

marauder, in addition to supporting multiple mutation syntaxes, also supports converting between them. The conversion is done by specifying the input and output syntaxes, and the tool will convert the mutations from the input syntax to the output syntax. The conversion is a crucial feature, as different mutation syntaxes have different trade-offs, and it is important to be able to switch between them. While git patches can allow writing mutations
as if they were changes to the code, they do not allow a holistic view of the mutations as the comment syntax, which requires lots of machinery to work with as opposed to the preprocessor macros, all of which are slower to use than the functional mutations due to the need for multiple compilations.


