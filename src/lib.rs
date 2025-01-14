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

pub mod algebra;
pub mod code;
pub mod languages;
pub mod syntax;
pub mod project;
pub mod variation;