/// Marauders is a library and command line tool for injecting amd maintaining inline mutations in source code.
/// 
/// The tool can be used targeting singular files as well as entire projects, analyzing files to identity
/// existing mutations, switching between them, and adding new ones.
/// 
/// The mutations use a comment-augmented syntax to identify the mutations and their variants.
/// 
/// ```rust
/// fn add(a: i32, b: i32) -> i32 {
///     /*| add_variation */
///     a + b
///     /*|| add_mutation_1 */
///     /*|
///     a - b
///     */
///     /*| add_mutation_2 */
///     /*|
///     a * b
///     */
///     /* |*/
/// }
/// ```
/// 
/// The users can invoke mutations by name, or a small DSL that expresses a set of mutations to apply. More details about the mutation
/// DSL can be found in the documentation of the `algebra` module.


/// The library is organized in the following modules:
/// 
/// * `algebra`: Contains the DSL for expressing mutations.
pub mod algebra;
/// * `cli`: Contains the command line interface for marauders.
pub mod cli;
/// * `code`: Contains the way marauders handle the code it analyzes and processes.
pub mod code;
/// * `languages`: Contains the language specific details for marauders supported languages.
pub mod languages;
/// * `syntax`: Contains the different syntaxes for expressing mutants.
pub mod syntax;
/// * `project`: Contains the logic and structures for handling marauders projects.
pub mod project;
/// * `variation`: Contains the logic and structures for about variations.
pub mod variation;