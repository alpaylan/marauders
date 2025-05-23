use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;
use marauders::{
    algebra, run_init_command, run_list_command, run_reset_command, run_set_command,
    run_unset_command, CustomLanguage, Language, ProjectConfig,
};

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let opts = Opts::parse();

    run(opts)
}

#[derive(Parser)]
pub(crate) struct Opts {
    #[command(subcommand)]
    command: Command,
}

#[derive(Parser)]
pub(crate) enum Command {
    #[clap(name = "list", about = "List variations in the code")]
    List {
        #[clap(short, long, default_value = ".")]
        path: PathBuf,
        #[clap(long)]
        pattern: Option<String>,
    },
    #[clap(name = "set", about = "Set active variant")]
    Set {
        #[clap(short, long, default_value = ".")]
        path: PathBuf,
        #[clap(short, long)]
        variant: String,
        #[clap(long)]
        pattern: Option<String>,
    },
    #[clap(name = "unset", about = "Unset active variant")]
    Unset {
        #[clap(short, long, default_value = ".")]
        path: PathBuf,
        #[clap(short, long)]
        variant: String,
    },
    #[clap(name = "reset", about = "Reset all variationts to base")]
    Reset {
        #[clap(short, long, default_value = ".")]
        path: PathBuf,
    },
    #[clap(name = "init", about = "Initialize a project")]
    Init {
        #[clap(short, long, default_value = ".")]
        path: PathBuf,
        #[clap(short, long)]
        language: String,
        #[clap(short, long)]
        use_gitignore: bool,
    },
    #[clap(subcommand, about = "Configure project")]
    Config(ConfigCommand),
    #[clap(name = "test", about = "Use marauders to run a series of tests")]
    Run {
        #[clap(short, long)]
        /// The mutation expression for running tests
        expr: String,
        #[clap(short, long, default_value = ".")]
        path: PathBuf,
        #[clap(short, long)]
        /// The test command to run
        /// The command should be a shell command that runs the tests
        command: String,
        #[clap(short, long, default_value = "false")]
        /// Whether to print the output or not
        nocapture: bool,
    },
}

#[derive(Parser)]
pub(crate) enum ConfigCommand {
    #[clap(name = "use-gitignore", about = "Use .gitignore for ignoring files")]
    UseGitignore {
        #[clap(short, long)]
        path: Option<PathBuf>,
    },
    #[clap(name = "add-ignore", about = "Add a pattern to ignore")]
    AddIgnore {
        #[clap(short, long)]
        path: Option<PathBuf>,
        #[clap(long)]
        pattern: String,
    },
    #[clap(name = "remove-ignore", about = "Remove a pattern from ignore")]
    RemoveIgnore {
        #[clap(short, long)]
        path: Option<PathBuf>,
        #[clap(short, long)]
        pattern: String,
    },
    #[clap(name = "add-language", about = "Add a language to the project")]
    AddLanguage {
        #[clap(short, long)]
        path: Option<PathBuf>,
        #[clap(short, long)]
        language: String,
    },
    #[clap(name = "remove-language", about = "Remove a language from the project")]
    RemoveLanguage {
        #[clap(short, long)]
        path: Option<PathBuf>,
        #[clap(short, long)]
        language: String,
    },
    #[clap(
        name = "add-custom-language",
        about = "Add a custom language to the project"
    )]
    AddCustomLanguage {
        #[clap(short, long)]
        path: Option<PathBuf>,
        #[clap(short, long)]
        name: String,
        #[clap(short = 'x', long)]
        extension: String,
        #[clap(short = 'b', long)]
        comment_begin: String,
        #[clap(short = 'e', long)]
        comment_end: String,
        #[clap(short = 'm', long)]
        mutation_marker: String,
    },
    #[clap(
        name = "remove-custom-language",
        about = "Remove a custom language from the project"
    )]
    RemoveCustomLanguage {
        #[clap(short, long)]
        path: Option<PathBuf>,
        #[clap(short, long)]
        name: String,
    },
    #[clap(name = "list", about = "List project configuration")]
    List {
        #[clap(short, long)]
        path: Option<PathBuf>,
    },
}

impl ConfigCommand {
    fn path(&self) -> Option<&Path> {
        match self {
            ConfigCommand::UseGitignore { path, .. }
            | ConfigCommand::AddIgnore { path, .. }
            | ConfigCommand::RemoveIgnore { path, .. }
            | ConfigCommand::AddLanguage { path, .. }
            | ConfigCommand::RemoveLanguage { path, .. }
            | ConfigCommand::AddCustomLanguage { path, .. }
            | ConfigCommand::RemoveCustomLanguage { path, .. }
            | ConfigCommand::List { path } => path.as_deref(),
        }
    }
}

pub(crate) fn run(opts: Opts) -> anyhow::Result<()> {
    match &opts.command {
        Command::List { path, pattern } => {
            log::info!("listing variations at '{}'", path.to_string_lossy());
            run_list_command(path, pattern.as_deref())?;
        }
        Command::Set { path, variant, pattern } => {
            log::info!(
                "set active variant '{}' at '{}'",
                variant,
                path.to_string_lossy()
            );
            run_set_command(path, variant, pattern.as_deref())?;
        }
        Command::Unset { path, variant } => {
            log::info!("unset active variant '{}'", variant);
            run_unset_command(path, variant)?;
        }
        Command::Reset { path } => {
            log::info!(
                "resetting all variations to base at '{}'",
                path.to_string_lossy()
            );
            run_reset_command(path)?;
        }
        Command::Init {
            path,
            language,
            use_gitignore,
        } => {
            log::info!("initializing project at '{}'", path.to_string_lossy());
            run_init_command(path, language, *use_gitignore)?;
        }
        Command::Config(config_command) => {
            log::info!("configuring project");
            run_config_command(config_command)?;
        }
        Command::Run {
            expr,
            path,
            command,
            nocapture,
        } => {
            log::info!("running tests at '{}'", path.to_string_lossy());
            run_run_command(expr, path, command, *nocapture)?;
        }
    }

    Ok(())
}

fn run_run_command(expr: &str, path: &Path, command: &str, nocapture: bool) -> anyhow::Result<()> {
    let path = path.to_path_buf();

    let mut project = marauders::Project::new(&path, None)?;

    let active_variants = project.active_variants();

    anyhow::ensure!(
        active_variants.is_empty(),
        r#"test command is only available when there are no active variants in the project, if you would like run the test command, please first run `marauders reset`."#
    );

    let tag_map = project.tag_map();
    let variation_map = project.variation_map();
    let variant_list = project.all_variants();

    let tests = algebra::compute_mutations(&expr, &tag_map, &variation_map, &variant_list)?;

    for test in tests {
        project.set_many(&test)?;
        let result = project.run(command);
        match result {
            Ok(output) => {
                if nocapture {
                    print!("{}", String::from_utf8_lossy(&output.stdout));
                    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
                }

                if output.status.success() {
                    println!("Test passed: {:?}", test);
                } else {
                    println!("Test failed: {:?}", test);
                }
            }
            Err(e) => {
                println!("Test failed: {:?}", test);
                println!("Error: {:?}", e);
            }
        }
    }

    project.reset()
}

fn run_config_command(config_command: &ConfigCommand) -> anyhow::Result<()> {
    let path = config_command
        .path()
        .map_or_else(|| std::env::current_dir().unwrap(), |p| p.to_path_buf());

    let cfg = fs::read_to_string(path.join("marauder.toml")).ok();

    let mut project_config = if let Some(cfg) = cfg {
        log::info!("found project config at '{}'", path.to_string_lossy());
        toml::from_str::<ProjectConfig>(&cfg)?
    } else {
        log::error!("project config not found at '{}'", path.to_string_lossy());
        return Err(anyhow::anyhow!("project config not found"));
    };

    match config_command {
        ConfigCommand::UseGitignore { .. } => {
            project_config.use_gitignore = true;
        }
        ConfigCommand::AddIgnore { pattern, .. } => {
            if !project_config.ignore.contains(pattern) {
                project_config.ignore.push(pattern.clone());
            }
        }
        ConfigCommand::RemoveIgnore { pattern, .. } => {
            project_config.ignore.retain(|p| p != pattern);
        }
        ConfigCommand::AddLanguage { language, .. } => {
            let language = Language::name_to_language(language, &project_config.custom_languages)
                .ok_or_else(|| anyhow::anyhow!("language '{language}' not found"))?;
            if !project_config.languages.contains(&language) {
                project_config.languages.push(language);
            }
        }
        ConfigCommand::RemoveLanguage { language, .. } => {
            let language = Language::name_to_language(language, &project_config.custom_languages)
                .ok_or_else(|| anyhow::anyhow!("language '{language}' not found"))?;
            project_config.languages.retain(|l| l != &language);
        }
        ConfigCommand::AddCustomLanguage {
            name,
            extension,
            comment_begin,
            comment_end,
            mutation_marker,
            ..
        } => {
            if project_config
                .custom_languages
                .iter()
                .any(|l| &l.name == name)
            {
                return Err(anyhow::anyhow!("language '{name}' already exists"));
            }

            project_config.custom_languages.push(CustomLanguage {
                name: name.clone(),
                extension: extension.clone(),
                comment_begin: comment_begin.clone(),
                comment_end: comment_end.clone(),
                mutation_marker: mutation_marker.clone(),
            });
        }
        ConfigCommand::RemoveCustomLanguage { name, .. } => {
            project_config.custom_languages.retain(|l| &l.name != name);
        }
        ConfigCommand::List { .. } => {
            println!("{}", toml::to_string(&project_config)?);
        }
    }

    fs::write(
        path.join("marauder.toml"),
        toml::to_string(&project_config)?,
    )?;

    log::info!("project config updated at '{}'", path.to_string_lossy());

    Ok(())
}
