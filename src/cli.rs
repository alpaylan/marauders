use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;

use crate::{
    code::SpanContent,
    languages::Language,
    project::{Project, ProjectConfig},
};

mod algebra;
mod code;
mod languages;
mod project;
mod syntax;
mod variation;

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
        #[clap(short, long)]
        path: PathBuf,
        #[clap(long)]
        pattern: Option<String>,
    },
    #[clap(name = "set", about = "Set active variant")]
    Set {
        #[clap(short, long)]
        path: PathBuf,
        #[clap(short, long)]
        variant: String,
    },
    #[clap(name = "unset", about = "Unset active variant")]
    Unset {
        #[clap(short, long)]
        path: PathBuf,
        #[clap(short, long)]
        variant: String,
    },
    #[clap(name = "reset", about = "Reset all variationts to base")]
    Reset {
        #[clap(short, long)]
        path: PathBuf,
    },
    #[clap(name = "init", about = "Initialize a project")]
    Init {
        #[clap(short, long)]
        path: PathBuf,
        #[clap(short, long)]
        language: String,
        #[clap(short, long)]
        use_gitignore: bool,
    },
    #[clap(subcommand, about = "Configure project")]
    Config(ConfigCommand),
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
        Command::Set { path, variant } => {
            log::info!(
                "set active variant '{}' at '{}'",
                variant,
                path.to_string_lossy()
            );
            run_set_command(path, variant)?;
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
    }

    Ok(())
}

fn run_list_command(path: &Path, pattern: Option<&str>) -> anyhow::Result<()> {
    // Check if there's a project config in the path
    let project = Project::new(path, pattern)?;

    for file in project.files.iter() {
        let code = &file.code;

        for span in code.spans.iter() {
            if let SpanContent::Variation(v) = &span.content {
                println!("{}:{} {}", file.path.to_string_lossy(), span.line, v);
            }
        }
    }

    Ok(())
}

fn run_set_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    // todo: check currently active variant, and do not set it again
    let project = Project::new(path, None)?;

    let mut found = false;
    let mut variants = vec![];
    for file in project.files.into_iter() {
        let mut code = file.code;
        if let Some((variation_index, variation)) =
            code.spans
                .iter()
                .enumerate()
                .find(|(_, v)| match &v.content {
                    SpanContent::Variation(v) => v.variants.iter().any(|v| v.name == variant),
                    _ => false,
                })
        {
            found = true;
            let variation = match &variation.content {
                SpanContent::Variation(v) => v,
                _ => unreachable!(),
            };

            let (variant_index, _) = variation
                .variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == variant)
                .ok_or_else(|| anyhow::anyhow!("variant not found"))?;

            // Shift index by because 0 is reserved for the base code
            let variant_index = variant_index + 1;

            log::info!(
                "variant index is '{}' at '({}, {})'",
                variant_index,
                variation.name.as_deref().unwrap_or("anonymous"),
                variation_index,
            );

            code.set_active_variant(variation_index, variant_index)?;

            log::info!("active variant set to '{}'", variant);
            println!("active variant set to '{}'", variant);
        } else {
            variants.extend(
                code.get_all_variants()
                    .into_iter()
                    .map(|v| (file.path.clone(), v)),
            );
        }
    }

    if !found {
        log::error!(
            "variant '{variant}' not found, possible variants are (\n{}\n)",
            variants
                .iter()
                .map(|(path, v)| format!("\t'{}' at '{}'", v, path.to_string_lossy()))
                .collect::<Vec<String>>()
                .join(",\n")
        )
    }

    Ok(())
}

fn run_unset_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    // todo: check currently active variant, if it is not set, do not unset it
    let project = Project::new(path, None)?;

    let mut found = false;
    let mut variants = vec![];

    for file in project.files.into_iter() {
        let mut code = file.code;
        if let Some((variation_index, variation)) =
            code.spans
                .iter()
                .enumerate()
                .find(|(_, v)| match &v.content {
                    SpanContent::Variation(v) => v.variants.iter().any(|v| v.name == variant),
                    _ => false,
                })
        {
            found = true;
            let variation = match &variation.content {
                SpanContent::Variation(v) => v,
                _ => unreachable!(),
            };

            log::info!(
                "variant is '({}, {})'",
                variation.name.as_deref().unwrap_or("anonymous"),
                variation_index,
            );

            code.set_active_variant(variation_index, 0)?;

            log::info!("active variant unset");
            println!("active variant unset");
        } else {
            variants.extend(
                code.get_all_variants()
                    .into_iter()
                    .map(|v| (file.path.clone(), v)),
            );
        }
    }

    Ok(())
}

fn run_reset_command(path: &Path) -> anyhow::Result<()> {

    let project = Project::new(path, None)?;

    for file in project.files.into_iter() {
        let mut code = file.code;
        code.spans.iter_mut().for_each(|span| {
            if let SpanContent::Variation(v) = &mut span.content {
                v.active = 0;
            }
        });

        code.save_to_file(&file.path)?;
    }

    log::info!("all variations reset to base");
    println!("all variations reset to base");

    Ok(())
}

fn run_init_command(path: &Path, language: &str, use_gitignore: bool) -> anyhow::Result<()> {
    let project_config = ProjectConfig {
        languages: Language::name_to_language(language, &vec![]).map_or(vec![], |l| vec![l]),
        custom_languages: vec![],
        ignore: vec![],
        use_gitignore,
    };

    fs::write(
        path.join("marauder.toml"),
        toml::to_string(&project_config)?,
    )?;

    log::info!("project initialized at '{}'", path.to_string_lossy());
    println!("project initialized at '{}'", path.to_string_lossy());

    Ok(())
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

            project_config
                .custom_languages
                .push(crate::languages::CustomLanguage {
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
    println!("project config updated at '{}'", path.to_string_lossy());

    Ok(())
}
