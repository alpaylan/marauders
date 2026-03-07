# Marauder

Marauder is a command line tool built for inline mutation testing. It is designed to be used in conjunction with a test suite to identify and apply mutations to code.

Current Capabilities:

- [x] Report the set of variants in a file.
- [x] Activate a variant in a file.
- [x] Deactivate a variant in a file.
- [x] Reset a file to its original state.
- [x] Run marauder on a directory.
- [x] Support mutation expressions[mutation-expressions]
- [ ] Run marauder on incremental mode[incremental-mode]
- [ ] Run marauder on copy mode[copy-mode]
- [x] Support C Preprocessor Macros[cpp-macros]
- [x] Support Functional Mutations[functional-mutations]
- [x] Support Git Patch Mutations[git-patch-mutations]
- [x] Support Match-and-Replace Mutations[match-replace-mutations]

[mutation-expressions]: #mutation-expressions
[functional-mutations]: #functional-mutations
[cpp-macros]: #preprocessor-macros
[git-patch-mutations]: #patch-mutations
[match-replace-mutations]: #match-and-replace-mutations

## Installation

From crates.io:

```bash
cargo install marauders
```

From GitHub Releases (prebuilt binaries for Linux/macOS):

```bash
curl -fsSL https://raw.githubusercontent.com/alpaylan/marauders/main/marauders-installer.sh | sh
```

This installs both `marauders` and `marauders-import-rust-mutants` into
`$HOME/.local/bin` by default.

## Embedding as a Library

`marauders` now exposes a smaller dependency surface for library users via Cargo
features.

- `default` features: `full`, `cli` (includes binaries and Rust AST conversion/import stack).
- `--no-default-features`: library-only build without CLI and Rust AST stack.
- `syntax-rust-functional`: enable Rust functional conversion support (pulls `syn`/`quote`/`proc-macro2`).
- `import-rust-mutants`: enable Rust mutant import validation stack.

Minimal embedding example:

```toml
[dependencies]
marauders = { version = "0.0.12", default-features = false }
```

Full tooling (existing behavior):

```toml
[dependencies]
marauders = { version = "0.0.12" }
```

## Usage

```bash
$ marauders --help
> 
Usage: marauders <COMMAND>
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
$ marauders list --path <path-to-file>
> 
    test/BST.v:21 (name: insert, active: base, variants: ["insert_1", "insert_2", "insert_3"], tags: ["new", "easy"])
    test/BST.v:57 (name: anonymous, active: base, variants: ["delete_4", "delete_5"], tags: [])
    test/BST.v:104 (name: anonymous, active: base, variants: ["union_6", "union_7", "union_8"], tags: [])
```

Users can set the active variant in a file or directory using the `set` command:

```bash
$ marauders set --path <path-to-file> --variant <variant-name>
> active variant set to 'insert_1' in 'test/BST.v:21'
```

Users can unset the active variant in a file or directory using the `unset` command:

```bash
$ marauders unset --path <path-to-file>
> active variant unset to base in 'test/BST.v:21'
```

Users can reset all variations in a file or directory using the `reset` command:

```bash
$ marauders reset --path <path-to-file>
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
        | variant
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

marauders supports multiple mechanisms for expressing mutations within code, the default
mode is the `comment syntax`, in which users can express mutations by adding comments
to the code. The comment syntax is as follows:

```rust
fn add(a: i32, b: i32) -> i32 {
    /*| add [arith, core] */
    a + b
    /*|| add_1 */
    /*|
    a - b
    */
    /*|| add_2 */
    /*|
    a * b
    */
    /* |*/
}
```

This code has 1 variation, named `add`, and 2 variants within the variation, named `add_1` and `add_2`. A Pest grammar of the syntax can be found at `src/syntax/comment.pest`. It is also possible to tag variations and variants with tags, as tags can be used to select specific subsets of mutations to apply.

### Preprocessor Macros

C preprocessor macros are a language independent way to express mutations in code. The syntax is as follows:

```c
int add(int a, int b) {
    #if defined(M_add_1) /* marauders:variation=add;tags=arith,core */
    return a - b;
    #elif defined(M_add_2)
    return a * b;
    #else
    return a + b;
    #endif
}
```

### Functional Mutations

Functional mutations are a mechanism for expressing mutations within code, using environment variables. The syntax is as follows:

```rust
fn add(a: i32, b: i32) -> i32 {
    /* marauders:variation=add;tags=arith,core */
    match () {
        _ if matches!(std::env::var("M_add_1").as_deref(), Ok("active")) => {
            a - b
        },
        _ if matches!(std::env::var("M_add_2").as_deref(), Ok("active")) => {
            a * b
        },
        _ => {
            a + b
        },
    }
}
```

Each variant is selected via an environment variable named `M_<variant>` set to `active`
(for example `M_add_1=active`). If no variant is active, execution falls back to the base
branch. A very
important benefit of this mechanism is that it does not require multiple compilation steps,
which is an issue with all other mutation types. Although, the downside is it is very intrusive within the code, reducing readability, and maintainability.

### Patch Mutations

Patch mutations are represented as a sidecar bundle:
- base program (mutations stripped) is written back to the original source file,
- patch metadata is written to `<source>.patches/manifest.toml`, and
- one unified diff file per variant is written under `<source>.patches/<variation>/`.

The manifest stores source path and variation tags so comment syntax can be reconstructed.
Converting that manifest back to comment syntax patches the source file referenced by `source`.

### Match-and-Replace Mutations

Match-and-replace mutations are represented as JSON documents that store
`scope` strings (`path:line` or `path:start-end`), a single `match` pattern, and
replacement snippets for each variation.

When converting comment syntax to match-replace, Marauders writes:
- base program (mutations stripped) back to the original source file, and
- mutation JSON to `<source>.match_replace.json`.

Converting that JSON back to comment syntax patches the source file referenced
by `scope`.

### Mutation Conversion

marauders, in addition to supporting multiple mutation syntaxes, also supports converting between them. The conversion is done by specifying the input and output syntaxes, and the tool will convert the mutations from the input syntax to the output syntax. The conversion is a crucial feature, as different mutation syntaxes have different trade-offs, and it is important to be able to switch between them. While git patches can allow writing mutations
as if they were changes to the code, they do not allow a holistic view of the mutations as the comment syntax, which requires lots of machinery to work with as opposed to the preprocessor macros, all of which are slower to use than the functional mutations due to the need for multiple compilations.

For Rust files, conversion between comment and functional syntax is available:

```bash
marauders convert --path test/rust/bst.rs --to functional
marauders convert --path test/rust/bst.rs --to comment
```

Language-agnostic conversion targets are also available:

```bash
marauders convert --path test/rust/bst.rs --to preprocessor
marauders convert --path test/rust/bst.rs --to patch
marauders convert --path test/rust/bst.rs --to match-replace
marauders convert --path test/rust/bst.rs --to comment
```

You can also import mutants generated by external tools (for example cargo-mutants output copies):
this functionality is provided by the separate `marauders-import-rust-mutants` executable,
not the main `marauders` binary.

```bash
marauders-import-rust-mutants \
  --base src/my_file.rs \
  --mutants-dir materialized_mutants \
  --prefix ext \
  --output src/my_file.imported.rs
```

For direct cargo-mutants output (diff files in `mutants.out`), use:

```bash
marauders-import-rust-mutants \
  --base src/my_file.rs \
  --cargo-mutants-dir mutants.out \
  --prefix cargo \
  --output src/my_file.imported.rs
```

Fully automated mode (only input file + output path):

```bash
marauders-import-rust-mutants \
  --base src/my_file.rs \
  --output src/my_file.imported.rs
```

When only `--base` (and optional `--output`) is provided, Marauders runs `cargo mutants`
in the containing Cargo project, captures its output, and imports generated Rust mutants.
This is the "just give input + output" flow.
If no `Cargo.toml` exists above the file, it creates a temporary single-file Cargo project
just for mutation generation.

If you also pass `--diffs`, generated cargo-mutants diff files are copied into a
`diffs/` folder in your current working directory.

Repository reproducible example:

```bash
marauders-import-rust-mutants \
  --base test/rust/cargo_mutants_demo/base/calc.rs \
  --cargo-mutants-dir test/rust/cargo_mutants_demo/mutants.out \
  --prefix tool \
  --output test/rust/cargo_mutants_imported_example.rs
```

An example imported file is available at `test/rust/cargo_mutants_imported_example.rs`.

The source used to generate that file via cargo-mutants is:
`test/rust/import_inputs/project/src/main.rs`

You can regenerate the example directly:

```bash
cargo run --bin marauders-import-rust-mutants -- \
  --base test/rust/import_inputs/project/src/main.rs \
  --output test/rust/cargo_mutants_imported_example.rs \
  --prefix tool
```
