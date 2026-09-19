use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use codex_core::config::edit::ConfigEdit;
use codex_core::config::edit::ConfigEditsBuilder;
use codex_core::config::find_codex_home;
use codex_models_manager::model_info::model_info_from_slug;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::protocol::MultiAgentVersion;
use codex_utils_path::write_atomically;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::read;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use toml_edit::Item as TomlItem;
use toml_edit::Table as TomlTable;
use toml_edit::value;
use url::Url;

const REGISTRY_VERSION: u32 = 1;
const MANAGED_DIR: &str = "codex-dp";
const REGISTRY_FILE: &str = "providers.toml";
const SECRETS_FILE: &str = "secrets.json";
const APPLIED_PROVIDERS_FILE: &str = "applied-providers.json";
const CATALOG_DIR: &str = "model-catalogs";
const ROUTES_FILE: &str = "routes.json";
const DEEPSEEK_CATALOG: &str =
    include_str!("../../../contrib/deepseek-subagents/deepseek-models.json");

#[derive(Debug, Parser)]
pub(crate) struct ProviderCli {
    #[command(subcommand)]
    command: ProviderCommand,
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    /// List managed providers and models.
    List(ListArgs),
    /// Add a provider. With no id, starts an interactive setup.
    Add(AddProviderArgs),
    /// Change an existing provider.
    Edit(EditProviderArgs),
    /// Remove a provider and all of its model routes.
    Remove(RemoveProviderArgs),
    /// Manage models belonging to a provider.
    Model(ModelCli),
    /// Manage locally stored provider API keys.
    Secret(SecretCli),
    /// Validate the registry without changing generated files.
    Validate,
    /// Check that a provider, its models, and its API key are ready locally.
    Test(TestProviderArgs),
    /// Regenerate config.toml, model catalogs, and routes.json.
    Apply,
    /// Install or refresh the built-in DeepSeek preset.
    #[command(hide = true)]
    InitDeepseek(InitDeepseekArgs),
}

#[derive(Debug, Args)]
struct ListArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Default)]
struct ProviderOptions {
    /// Friendly provider name.
    #[arg(long)]
    name: Option<String>,
    /// Base URL of an OpenAI Responses-compatible endpoint.
    #[arg(long)]
    base_url: Option<String>,
    /// Environment variable used for this provider's API key.
    #[arg(long)]
    env_key: Option<String>,
    /// Enable the Responses WebSocket transport.
    #[arg(long)]
    supports_websockets: Option<bool>,
}

#[derive(Debug, Args)]
struct AddProviderArgs {
    /// Stable provider id (letters, digits, underscore, and dash).
    id: Option<String>,
    #[command(flatten)]
    options: ProviderOptions,
    /// Add a model route as PUBLIC_ID=API_MODEL. Repeat to add multiple models.
    #[arg(long = "model", value_name = "PUBLIC_ID=API_MODEL")]
    models: Vec<String>,
    /// Store the key currently held in this environment variable.
    #[arg(long, value_name = "SOURCE_ENV")]
    store_key_from_env: Option<String>,
}

#[derive(Debug, Args)]
struct EditProviderArgs {
    id: String,
    #[command(flatten)]
    options: ProviderOptions,
}

#[derive(Debug, Args)]
struct RemoveProviderArgs {
    id: String,
    /// Keep any locally stored secret for the provider's env key.
    #[arg(long)]
    keep_secret: bool,
}

#[derive(Debug, Parser)]
struct ModelCli {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Debug, Subcommand)]
enum ModelCommand {
    /// List managed models, optionally for one provider.
    List(ModelListArgs),
    /// Add a model to a provider.
    Add(AddModelArgs),
    /// Change a model.
    Edit(EditModelArgs),
    /// Remove a model.
    Remove(RemoveModelArgs),
}

#[derive(Debug, Args)]
struct ModelListArgs {
    provider: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AddModelArgs {
    provider: String,
    /// Public model id shown to agents and in model pickers.
    id: String,
    /// Model name sent to the provider. Defaults to the public id.
    #[arg(long)]
    api_model: Option<String>,
    #[command(flatten)]
    options: ModelOptions,
}

#[derive(Debug, Args)]
struct EditModelArgs {
    provider: String,
    id: String,
    #[arg(long)]
    api_model: Option<String>,
    #[command(flatten)]
    options: ModelOptions,
}

#[derive(Debug, Args)]
struct RemoveModelArgs {
    provider: String,
    id: String,
}

#[derive(Debug, Args, Default)]
struct ModelOptions {
    #[arg(long)]
    display_name: Option<String>,
    #[arg(long)]
    description: Option<String>,
    /// Comma-separated reasoning efforts, for example low,high,max.
    #[arg(long, value_delimiter = ',')]
    reasoning_efforts: Vec<String>,
    #[arg(long)]
    default_reasoning_effort: Option<String>,
    #[arg(long)]
    context_window: Option<i64>,
    #[arg(long)]
    supports_images: Option<bool>,
}

#[derive(Debug, Parser)]
struct SecretCli {
    #[command(subcommand)]
    command: SecretCommand,
}

#[derive(Debug, Subcommand)]
enum SecretCommand {
    /// List stored environment-variable names. Values are never printed.
    List,
    /// Store a key from stdin or another environment variable.
    Set(SecretSetArgs),
    /// Delete a stored key.
    Remove(SecretRemoveArgs),
}

#[derive(Debug, Args)]
struct SecretSetArgs {
    env_key: String,
    /// Read the value from this environment variable.
    #[arg(long, conflicts_with = "stdin")]
    from_env: Option<String>,
    /// Read the value from stdin.
    #[arg(long)]
    stdin: bool,
}

#[derive(Debug, Args)]
struct SecretRemoveArgs {
    env_key: String,
}

#[derive(Debug, Args)]
struct InitDeepseekArgs {
    /// Store DEEPSEEK_API_KEY from this environment variable.
    #[arg(long)]
    store_key_from_env: Option<String>,
}

#[derive(Debug, Args)]
struct TestProviderArgs {
    id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProviderRegistry {
    #[serde(default = "registry_version")]
    schema_version: u32,
    #[serde(default)]
    providers: BTreeMap<String, ManagedProvider>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManagedProvider {
    name: String,
    base_url: String,
    env_key: String,
    #[serde(default)]
    supports_websockets: bool,
    #[serde(default)]
    models: Vec<ManagedModel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManagedModel {
    id: String,
    api_model: String,
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    reasoning_efforts: Vec<String>,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
    #[serde(default)]
    supports_images: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template: Option<String>,
}

#[derive(Debug, Serialize)]
struct RouteFile {
    version: u32,
    models: BTreeMap<String, RouteEntry>,
}

#[derive(Debug, Serialize)]
struct RouteEntry {
    provider: String,
    api_model: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AppliedProviders {
    version: u32,
    providers: Vec<String>,
}

const fn registry_version() -> u32 {
    REGISTRY_VERSION
}

pub(crate) fn load_managed_secrets() -> anyhow::Result<()> {
    let Ok(codex_home) = find_codex_home() else {
        return Ok(());
    };
    let path = secrets_path(&codex_home);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let secrets: BTreeMap<String, String> = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    for (key, secret) in secrets {
        validate_env_key(&key)?;
        if std::env::var_os(&key).is_none() {
            // SAFETY: this runs before the CLI creates worker threads. Existing process
            // environment values always win over managed values.
            unsafe { std::env::set_var(key, secret) };
        }
    }
    Ok(())
}

pub(crate) async fn run(cli: ProviderCli) -> anyhow::Result<()> {
    let codex_home = find_codex_home()?;
    match cli.command {
        ProviderCommand::List(args) => list_providers(&codex_home, args),
        ProviderCommand::Add(args) => add_provider(&codex_home, args).await,
        ProviderCommand::Edit(args) => edit_provider(&codex_home, args).await,
        ProviderCommand::Remove(args) => remove_provider(&codex_home, args).await,
        ProviderCommand::Model(cli) => run_model(&codex_home, cli).await,
        ProviderCommand::Secret(cli) => run_secret(&codex_home, cli),
        ProviderCommand::Validate => {
            let registry = load_registry(&codex_home)?;
            validate_registry(&registry)?;
            println!("Provider configuration is valid.");
            Ok(())
        }
        ProviderCommand::Test(args) => test_provider(&codex_home, args),
        ProviderCommand::Apply => {
            let registry = load_registry(&codex_home)?;
            apply_registry(&codex_home, &registry).await?;
            println!("Applied {} provider(s).", registry.providers.len());
            Ok(())
        }
        ProviderCommand::InitDeepseek(args) => init_deepseek(&codex_home, args).await,
    }
}

fn test_provider(codex_home: &Path, args: TestProviderArgs) -> anyhow::Result<()> {
    let registry = load_registry(codex_home)?;
    validate_registry(&registry)?;
    let provider = registry
        .providers
        .get(&args.id)
        .with_context(|| format!("provider `{}` is not managed", args.id))?;
    if provider.models.is_empty() {
        anyhow::bail!("provider `{}` has no configured models", args.id);
    }
    let secret = std::env::var(&provider.env_key).with_context(|| {
        format!(
            "provider `{}` needs API key environment variable `{}`",
            args.id, provider.env_key
        )
    })?;
    if secret.is_empty() {
        anyhow::bail!(
            "provider `{}` API key environment variable `{}` is empty",
            args.id,
            provider.env_key
        );
    }
    println!(
        "Provider `{}` is ready locally with {} model(s). No API request was sent.",
        args.id,
        provider.models.len()
    );
    Ok(())
}

fn list_providers(codex_home: &Path, args: ListArgs) -> anyhow::Result<()> {
    let registry = load_registry(codex_home)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&registry)?);
        return Ok(());
    }
    if registry.providers.is_empty() {
        println!("No managed providers configured.");
        return Ok(());
    }
    for (id, provider) in registry.providers {
        println!(
            "{id}  {}  {}  env={}  models={}",
            provider.name,
            provider.base_url,
            provider.env_key,
            provider.models.len()
        );
        for model in provider.models {
            println!("  {} -> {}", model.id, model.api_model);
        }
    }
    Ok(())
}

async fn add_provider(codex_home: &Path, args: AddProviderArgs) -> anyhow::Result<()> {
    let mut registry = load_registry(codex_home)?;
    let interactive = args.id.is_none();
    let id = match args.id {
        Some(id) => id,
        None => prompt_required("Provider id", None)?,
    };
    validate_provider_id(&id)?;
    if registry.providers.contains_key(&id) {
        anyhow::bail!("provider `{id}` already exists; use `codex provider edit {id}`");
    }
    let name = value_or_prompt(args.options.name, interactive, "Display name", &id)?;
    let base_url = match args.options.base_url {
        Some(base_url) => base_url,
        None if interactive => {
            prompt_required("Responses API base URL", Some("https://api.example.com/v1"))?
        }
        None => anyhow::bail!("--base-url is required for non-interactive setup"),
    };
    let env_default = format!("{}_API_KEY", id.replace('-', "_").to_ascii_uppercase());
    let env_key = value_or_prompt(
        args.options.env_key,
        interactive,
        "API key environment variable",
        &env_default,
    )?;
    let mut models = parse_model_pairs(&args.models)?;
    if interactive && models.is_empty() {
        loop {
            let public_id = prompt("Public model id (blank to finish)", None)?;
            if public_id.is_empty() {
                break;
            }
            let api_model = prompt_required("Provider API model", Some(&public_id))?;
            let mut model = default_model(public_id, api_model);
            let reasoning_efforts = prompt(
                "Supported reasoning efforts, comma-separated (blank for none)",
                None,
            )?;
            if !reasoning_efforts.is_empty() {
                model.reasoning_efforts = reasoning_efforts
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                let default_effort = prompt("Default reasoning effort (blank for none)", None)?;
                if !default_effort.is_empty() {
                    model.default_reasoning_effort = Some(default_effort);
                }
            }
            let context_window = prompt("Context window", Some("128000"))?;
            model.context_window = Some(
                context_window
                    .parse::<i64>()
                    .with_context(|| format!("invalid context window `{context_window}`"))?,
            );
            let supports_images = prompt("Supports image input?", Some("N"))?;
            model.supports_images =
                matches!(supports_images.to_ascii_lowercase().as_str(), "y" | "yes");
            models.push(model);
            let add_another = prompt("Add another model?", Some("N"))?;
            if !matches!(add_another.to_ascii_lowercase().as_str(), "y" | "yes") {
                break;
            }
        }
    }
    let provider = ManagedProvider {
        name,
        base_url,
        env_key: env_key.clone(),
        supports_websockets: args.options.supports_websockets.unwrap_or(false),
        models,
    };
    registry.providers.insert(id.clone(), provider);
    if let Some(source) = args.store_key_from_env {
        store_secret_from_env(codex_home, &env_key, &source)?;
    } else if interactive {
        let answer = prompt("Store this provider's API key now?", Some("Y"))?;
        if !matches!(answer.to_ascii_lowercase().as_str(), "n" | "no") {
            let secret = read_hidden_secret("API key (input hidden)")?;
            if !secret.is_empty() {
                set_secret(codex_home, &env_key, secret)?;
            }
        }
    }
    persist_and_apply(codex_home, &registry).await?;
    println!("Added provider `{id}`.");
    Ok(())
}

async fn edit_provider(codex_home: &Path, args: EditProviderArgs) -> anyhow::Result<()> {
    let mut registry = load_registry(codex_home)?;
    let provider = registry
        .providers
        .get_mut(&args.id)
        .with_context(|| format!("provider `{}` is not managed", args.id))?;
    if let Some(name) = args.options.name {
        provider.name = name;
    }
    if let Some(base_url) = args.options.base_url {
        provider.base_url = base_url;
    }
    if let Some(env_key) = args.options.env_key {
        provider.env_key = env_key;
    }
    if let Some(supports_websockets) = args.options.supports_websockets {
        provider.supports_websockets = supports_websockets;
    }
    persist_and_apply(codex_home, &registry).await?;
    println!("Updated provider `{}`.", args.id);
    Ok(())
}

async fn remove_provider(codex_home: &Path, args: RemoveProviderArgs) -> anyhow::Result<()> {
    let mut registry = load_registry(codex_home)?;
    let provider = registry
        .providers
        .remove(&args.id)
        .with_context(|| format!("provider `{}` is not managed", args.id))?;
    save_registry(codex_home, &registry)?;
    remove_provider_artifacts(codex_home, &args.id, &registry).await?;
    if !args.keep_secret {
        remove_secret(codex_home, &provider.env_key)?;
    }
    println!("Removed provider `{}`.", args.id);
    Ok(())
}

async fn run_model(codex_home: &Path, cli: ModelCli) -> anyhow::Result<()> {
    let mut registry = load_registry(codex_home)?;
    match cli.command {
        ModelCommand::List(args) => {
            let models: Vec<(&str, &ManagedModel)> = registry
                .providers
                .iter()
                .filter(|(id, _)| args.provider.as_ref().is_none_or(|wanted| wanted == *id))
                .flat_map(|(id, provider)| {
                    provider
                        .models
                        .iter()
                        .map(move |model| (id.as_str(), model))
                })
                .collect();
            if args.json {
                println!("{}", serde_json::to_string_pretty(&models)?);
            } else {
                for (provider, model) in models {
                    println!("{provider}  {} -> {}", model.id, model.api_model);
                }
            }
            Ok(())
        }
        ModelCommand::Add(args) => {
            let model = model_from_args(
                args.id,
                args.api_model,
                args.options,
                /*existing*/ None,
            )?;
            ensure_public_model_id_available(&registry, &model.id, None)?;
            let provider = registry
                .providers
                .get_mut(&args.provider)
                .with_context(|| format!("provider `{}` is not managed", args.provider))?;
            provider.models.push(model);
            persist_and_apply(codex_home, &registry).await?;
            println!("Added model to provider `{}`.", args.provider);
            Ok(())
        }
        ModelCommand::Edit(args) => {
            let existing = registry
                .providers
                .get(&args.provider)
                .with_context(|| format!("provider `{}` is not managed", args.provider))?
                .models
                .iter()
                .find(|model| model.id == args.id)
                .cloned()
                .with_context(|| format!("model `{}` is not managed", args.id))?;
            let model = model_from_args(
                args.id.clone(),
                args.api_model,
                args.options,
                Some(existing),
            )?;
            let provider = registry
                .providers
                .get_mut(&args.provider)
                .expect("provider was checked above");
            let slot = provider
                .models
                .iter_mut()
                .find(|candidate| candidate.id == args.id)
                .expect("model was checked above");
            *slot = model;
            persist_and_apply(codex_home, &registry).await?;
            println!("Updated model `{}`.", args.id);
            Ok(())
        }
        ModelCommand::Remove(args) => {
            let provider = registry
                .providers
                .get_mut(&args.provider)
                .with_context(|| format!("provider `{}` is not managed", args.provider))?;
            let before = provider.models.len();
            provider.models.retain(|model| model.id != args.id);
            if provider.models.len() == before {
                anyhow::bail!("model `{}` is not managed", args.id);
            }
            persist_and_apply(codex_home, &registry).await?;
            println!("Removed model `{}`.", args.id);
            Ok(())
        }
    }
}

fn run_secret(codex_home: &Path, cli: SecretCli) -> anyhow::Result<()> {
    match cli.command {
        SecretCommand::List => {
            for key in load_secrets(codex_home)?.keys() {
                println!("{key}");
            }
            Ok(())
        }
        SecretCommand::Set(args) => {
            validate_env_key(&args.env_key)?;
            let secret = if let Some(source) = args.from_env {
                std::env::var(&source)
                    .with_context(|| format!("environment variable `{source}` is not set"))?
            } else if args.stdin || !std::io::stdin().is_terminal() {
                let mut secret = String::new();
                std::io::stdin().read_to_string(&mut secret)?;
                secret.trim_end_matches(['\r', '\n']).to_string()
            } else {
                anyhow::bail!("use `--from-env NAME` or pipe the key with `--stdin`");
            };
            set_secret(codex_home, &args.env_key, secret)?;
            println!("Stored `{}`. The value was not printed.", args.env_key);
            Ok(())
        }
        SecretCommand::Remove(args) => {
            remove_secret(codex_home, &args.env_key)?;
            println!("Removed stored secret `{}`.", args.env_key);
            Ok(())
        }
    }
}

async fn init_deepseek(codex_home: &Path, args: InitDeepseekArgs) -> anyhow::Result<()> {
    let mut registry = load_registry(codex_home)?;
    registry
        .providers
        .insert("deepseek".to_string(), deepseek_provider()?);
    if let Some(source) = args.store_key_from_env {
        store_secret_from_env(codex_home, "DEEPSEEK_API_KEY", &source)?;
    }
    persist_and_apply(codex_home, &registry).await?;
    println!("DeepSeek provider preset is ready.");
    Ok(())
}

fn deepseek_provider() -> anyhow::Result<ManagedProvider> {
    let catalog: ModelsResponse = serde_json::from_str(DEEPSEEK_CATALOG)?;
    let models = catalog
        .models
        .into_iter()
        .map(|model| ManagedModel {
            id: model.slug.clone(),
            api_model: model.slug.clone(),
            display_name: model.display_name,
            description: model.description.unwrap_or_default(),
            reasoning_efforts: model
                .supported_reasoning_levels
                .into_iter()
                .map(|preset| preset.effort.to_string())
                .collect(),
            default_reasoning_effort: model
                .default_reasoning_level
                .map(|effort| effort.to_string()),
            context_window: model.context_window,
            supports_images: model.input_modalities.contains(&InputModality::Image),
            template: Some(model.slug),
        })
        .collect();
    Ok(ManagedProvider {
        name: "DeepSeek".to_string(),
        base_url: "https://api.deepseek.com/".to_string(),
        env_key: "DEEPSEEK_API_KEY".to_string(),
        supports_websockets: false,
        models,
    })
}

async fn persist_and_apply(codex_home: &Path, registry: &ProviderRegistry) -> anyhow::Result<()> {
    validate_registry(registry)?;
    save_registry(codex_home, registry)?;
    apply_registry(codex_home, registry).await
}

async fn apply_registry(codex_home: &Path, registry: &ProviderRegistry) -> anyhow::Result<()> {
    validate_registry(registry)?;
    let catalog_dir = codex_home.join(CATALOG_DIR);
    std::fs::create_dir_all(&catalog_dir)?;
    let mut routes = BTreeMap::new();
    let mut config_edits = Vec::new();
    let current_provider_ids = registry.providers.keys().cloned().collect::<BTreeSet<_>>();
    for provider_id in load_applied_providers(codex_home)?.providers {
        validate_provider_id(&provider_id)?;
        if !current_provider_ids.contains(&provider_id) {
            remove_file_if_present(&catalog_dir.join(format!("{provider_id}.json")))?;
            config_edits.push(ConfigEdit::ClearPath {
                segments: vec!["model_providers".to_string(), provider_id],
            });
        }
    }

    for (provider_id, provider) in &registry.providers {
        let catalog = ModelsResponse {
            models: provider
                .models
                .iter()
                .map(build_model_info)
                .collect::<anyhow::Result<Vec<_>>>()?,
        };
        write_json_atomically(&catalog_dir.join(format!("{provider_id}.json")), &catalog)?;
        for model in &provider.models {
            routes.insert(
                model.id.clone(),
                RouteEntry {
                    provider: provider_id.clone(),
                    api_model: model.api_model.clone(),
                },
            );
        }
        config_edits.push(ConfigEdit::SetPath {
            segments: vec!["model_providers".to_string(), provider_id.clone()],
            value: provider_toml(provider),
        });
    }
    write_json_atomically(
        &catalog_dir.join(ROUTES_FILE),
        &RouteFile {
            version: REGISTRY_VERSION,
            models: routes,
        },
    )?;
    ConfigEditsBuilder::new(codex_home)
        .with_edits(config_edits)
        .apply()
        .await?;
    save_applied_providers(codex_home, registry)
}

async fn remove_provider_artifacts(
    codex_home: &Path,
    provider_id: &str,
    registry: &ProviderRegistry,
) -> anyhow::Result<()> {
    let path = codex_home
        .join(CATALOG_DIR)
        .join(format!("{provider_id}.json"));
    remove_file_if_present(&path)?;
    let mut routes = BTreeMap::new();
    for (id, provider) in &registry.providers {
        for model in &provider.models {
            routes.insert(
                model.id.clone(),
                RouteEntry {
                    provider: id.clone(),
                    api_model: model.api_model.clone(),
                },
            );
        }
    }
    write_json_atomically(
        &codex_home.join(CATALOG_DIR).join(ROUTES_FILE),
        &RouteFile {
            version: REGISTRY_VERSION,
            models: routes,
        },
    )?;
    ConfigEditsBuilder::new(codex_home)
        .with_edits([ConfigEdit::ClearPath {
            segments: vec!["model_providers".to_string(), provider_id.to_string()],
        }])
        .apply()
        .await?;
    save_applied_providers(codex_home, registry)
}

fn build_model_info(model: &ManagedModel) -> anyhow::Result<ModelInfo> {
    let mut info = if let Some(template) = &model.template {
        let catalog: ModelsResponse = serde_json::from_str(DEEPSEEK_CATALOG)?;
        catalog
            .models
            .into_iter()
            .find(|candidate| candidate.slug == *template)
            .with_context(|| format!("unknown bundled model template `{template}`"))?
    } else {
        model_info_from_slug(&model.api_model)
    };
    info.slug.clone_from(&model.api_model);
    info.display_name.clone_from(&model.display_name);
    info.description = (!model.description.is_empty()).then(|| model.description.clone());
    info.visibility = ModelVisibility::List;
    info.multi_agent_version = Some(MultiAgentVersion::V2);
    info.context_window = model.context_window.or(info.context_window);
    info.max_context_window = info.context_window;
    info.input_modalities = if model.supports_images {
        vec![InputModality::Text, InputModality::Image]
    } else {
        vec![InputModality::Text]
    };
    info.supported_reasoning_levels = model
        .reasoning_efforts
        .iter()
        .map(|effort| {
            Ok(ReasoningEffortPreset {
                effort: effort.parse().map_err(anyhow::Error::msg)?,
                description: format!("{effort} reasoning effort"),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    info.default_reasoning_level = model
        .default_reasoning_effort
        .as_deref()
        .map(str::parse::<ReasoningEffort>)
        .transpose()
        .map_err(anyhow::Error::msg)?;
    Ok(info)
}

fn provider_toml(provider: &ManagedProvider) -> TomlItem {
    let mut table = TomlTable::new();
    table["name"] = value(provider.name.clone());
    table["base_url"] = value(provider.base_url.clone());
    table["env_key"] = value(provider.env_key.clone());
    table["wire_api"] = value("responses");
    table["supports_websockets"] = value(provider.supports_websockets);
    TomlItem::Table(table)
}

fn validate_registry(registry: &ProviderRegistry) -> anyhow::Result<()> {
    if registry.schema_version != REGISTRY_VERSION {
        anyhow::bail!(
            "unsupported provider registry version {}; expected {}",
            registry.schema_version,
            REGISTRY_VERSION
        );
    }
    let mut routes = BTreeMap::<&str, &str>::new();
    for (id, provider) in &registry.providers {
        validate_provider_id(id)?;
        validate_env_key(&provider.env_key)?;
        let url = Url::parse(&provider.base_url)
            .with_context(|| format!("provider `{id}` has an invalid base URL"))?;
        let is_local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
        if url.scheme() != "https" && !(url.scheme() == "http" && is_local) {
            anyhow::bail!("provider `{id}` must use HTTPS (HTTP is allowed only for localhost)");
        }
        for model in &provider.models {
            if model.id.trim().is_empty() || model.api_model.trim().is_empty() {
                anyhow::bail!("provider `{id}` has a model with an empty id or API model");
            }
            if let Some(previous) = routes.insert(&model.id, id) {
                anyhow::bail!(
                    "public model id `{}` is used by both `{previous}` and `{id}`",
                    model.id
                );
            }
            let efforts = model
                .reasoning_efforts
                .iter()
                .map(|effort| {
                    effort
                        .parse::<ReasoningEffort>()
                        .map_err(anyhow::Error::msg)
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            if let Some(default) = &model.default_reasoning_effort {
                let default = default
                    .parse::<ReasoningEffort>()
                    .map_err(anyhow::Error::msg)?;
                if !efforts.contains(&default) {
                    anyhow::bail!(
                        "model `{}` default reasoning effort must be in reasoning_efforts",
                        model.id
                    );
                }
            }
            if model.context_window.is_some_and(|value| value <= 0) {
                anyhow::bail!("model `{}` context window must be positive", model.id);
            }
        }
    }
    Ok(())
}

fn model_from_args(
    id: String,
    api_model: Option<String>,
    options: ModelOptions,
    existing: Option<ManagedModel>,
) -> anyhow::Result<ManagedModel> {
    let fallback = existing.unwrap_or_else(|| default_model(id.clone(), id.clone()));
    let reasoning_efforts = if options.reasoning_efforts.is_empty() {
        fallback.reasoning_efforts
    } else {
        options.reasoning_efforts
    };
    Ok(ManagedModel {
        id,
        api_model: api_model.unwrap_or(fallback.api_model),
        display_name: options.display_name.unwrap_or(fallback.display_name),
        description: options.description.unwrap_or(fallback.description),
        reasoning_efforts,
        default_reasoning_effort: options
            .default_reasoning_effort
            .or(fallback.default_reasoning_effort),
        context_window: options.context_window.or(fallback.context_window),
        supports_images: options.supports_images.unwrap_or(fallback.supports_images),
        template: fallback.template,
    })
}

fn default_model(id: String, api_model: String) -> ManagedModel {
    ManagedModel {
        display_name: id.clone(),
        id,
        api_model,
        description: String::new(),
        reasoning_efforts: Vec::new(),
        default_reasoning_effort: None,
        context_window: Some(128_000),
        supports_images: false,
        template: None,
    }
}

fn parse_model_pairs(values: &[String]) -> anyhow::Result<Vec<ManagedModel>> {
    values
        .iter()
        .map(|value| {
            let (id, api_model) = value.split_once('=').with_context(|| {
                format!("invalid model `{value}`; expected PUBLIC_ID=API_MODEL")
            })?;
            Ok(default_model(
                id.trim().to_string(),
                api_model.trim().to_string(),
            ))
        })
        .collect()
}

fn ensure_public_model_id_available(
    registry: &ProviderRegistry,
    model_id: &str,
    except_provider: Option<&str>,
) -> anyhow::Result<()> {
    for (provider_id, provider) in &registry.providers {
        if Some(provider_id.as_str()) != except_provider
            && provider.models.iter().any(|model| model.id == model_id)
        {
            anyhow::bail!("public model id `{model_id}` is already configured");
        }
    }
    Ok(())
}

fn validate_provider_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        anyhow::bail!("invalid provider id `{id}`; use only letters, digits, underscore, and dash");
    }
    if id == "openai" {
        anyhow::bail!("the built-in OpenAI provider id cannot be managed here");
    }
    Ok(())
}

fn validate_env_key(key: &str) -> anyhow::Result<()> {
    let mut chars = key.chars();
    let valid_first = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if !valid_first || !chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        anyhow::bail!("invalid environment variable name `{key}`");
    }
    Ok(())
}

fn value_or_prompt(
    value: Option<String>,
    interactive: bool,
    label: &str,
    default: &str,
) -> anyhow::Result<String> {
    match value {
        Some(value) => Ok(value),
        None if interactive => prompt_required(label, Some(default)),
        None => Ok(default.to_string()),
    }
}

fn prompt_required(label: &str, default: Option<&str>) -> anyhow::Result<String> {
    let value = prompt(label, default)?;
    if value.is_empty() {
        anyhow::bail!("{label} must not be empty");
    }
    Ok(value)
}

fn prompt(label: &str, default: Option<&str>) -> anyhow::Result<String> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("interactive setup requires a terminal; pass a provider id and options");
    }
    match default {
        Some(default) => print!("{label} [{default}]: "),
        None => print!("{label}: "),
    }
    std::io::stdout().flush()?;
    let mut value = String::new();
    std::io::stdin().read_line(&mut value)?;
    let value = value.trim().to_string();
    Ok(if value.is_empty() {
        default.unwrap_or_default().to_string()
    } else {
        value
    })
}

fn read_hidden_secret(label: &str) -> anyhow::Result<String> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("hidden API key input requires a terminal");
    }
    print!("{label}: ");
    std::io::stdout().flush()?;
    enable_raw_mode()?;
    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }
    let guard = RawModeGuard;
    let mut secret = String::new();
    loop {
        let Event::Key(key) = read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Enter => break,
            KeyCode::Backspace => {
                secret.pop();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                drop(guard);
                println!();
                anyhow::bail!("API key input cancelled");
            }
            KeyCode::Char(character) => secret.push(character),
            _ => {}
        }
    }
    drop(guard);
    println!();
    Ok(secret)
}

fn registry_path(codex_home: &Path) -> PathBuf {
    codex_home.join(MANAGED_DIR).join(REGISTRY_FILE)
}

fn secrets_path(codex_home: &Path) -> PathBuf {
    codex_home.join(MANAGED_DIR).join(SECRETS_FILE)
}

fn applied_providers_path(codex_home: &Path) -> PathBuf {
    codex_home.join(MANAGED_DIR).join(APPLIED_PROVIDERS_FILE)
}

fn load_registry(codex_home: &Path) -> anyhow::Result<ProviderRegistry> {
    let path = registry_path(codex_home);
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            toml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ProviderRegistry {
            schema_version: REGISTRY_VERSION,
            providers: BTreeMap::new(),
        }),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn save_registry(codex_home: &Path, registry: &ProviderRegistry) -> anyhow::Result<()> {
    let path = registry_path(codex_home);
    ensure_parent(&path)?;
    let contents = toml::to_string_pretty(registry)?;
    write_atomically(&path, &contents)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn load_applied_providers(codex_home: &Path) -> anyhow::Result<AppliedProviders> {
    let path = applied_providers_path(codex_home);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AppliedProviders::default()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn save_applied_providers(codex_home: &Path, registry: &ProviderRegistry) -> anyhow::Result<()> {
    write_json_atomically(
        &applied_providers_path(codex_home),
        &AppliedProviders {
            version: REGISTRY_VERSION,
            providers: registry.providers.keys().cloned().collect(),
        },
    )
}

fn remove_file_if_present(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn load_secrets(codex_home: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let path = secrets_path(codex_home);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn set_secret(codex_home: &Path, env_key: &str, secret: String) -> anyhow::Result<()> {
    if secret.is_empty() {
        anyhow::bail!("API key must not be empty");
    }
    let mut secrets = load_secrets(codex_home)?;
    secrets.insert(env_key.to_string(), secret);
    save_secrets(codex_home, &secrets)
}

fn store_secret_from_env(codex_home: &Path, env_key: &str, source: &str) -> anyhow::Result<()> {
    let secret = std::env::var(source)
        .with_context(|| format!("environment variable `{source}` is not set"))?;
    set_secret(codex_home, env_key, secret)
}

fn remove_secret(codex_home: &Path, env_key: &str) -> anyhow::Result<()> {
    let mut secrets = load_secrets(codex_home)?;
    if secrets.remove(env_key).is_some() {
        save_secrets(codex_home, &secrets)?;
    }
    Ok(())
}

fn save_secrets(codex_home: &Path, secrets: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let path = secrets_path(codex_home);
    ensure_parent(&path)?;
    let contents = serde_json::to_string_pretty(secrets)?;
    write_atomically(&path, &contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    restrict_secret_permissions(&path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_secret_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_secret_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    ensure_parent(path)?;
    let contents = serde_json::to_string_pretty(value)?;
    write_atomically(path, &contents).with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_public_model_ids_are_rejected() {
        let mut registry = ProviderRegistry {
            schema_version: REGISTRY_VERSION,
            providers: BTreeMap::new(),
        };
        for id in ["one", "two"] {
            registry.providers.insert(
                id.to_string(),
                ManagedProvider {
                    name: id.to_string(),
                    base_url: format!("https://{id}.example.com/v1"),
                    env_key: format!("{}_API_KEY", id.to_ascii_uppercase()),
                    supports_websockets: false,
                    models: vec![default_model("shared".to_string(), "api-model".to_string())],
                },
            );
        }
        assert!(validate_registry(&registry).is_err());
    }

    #[test]
    fn deepseek_preset_preserves_two_models() {
        let provider = deepseek_provider().expect("deepseek preset should parse");
        assert_eq!(provider.models.len(), 2);
        assert!(
            provider
                .models
                .iter()
                .any(|model| model.id == "deepseek-flash")
        );
    }

    #[test]
    fn managed_secrets_round_trip_without_printing_values() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        set_secret(home.path(), "EXAMPLE_API_KEY", "secret-value".to_string())
            .expect("secret should be stored");
        let secrets = load_secrets(home.path()).expect("secrets should load");
        assert_eq!(
            secrets.get("EXAMPLE_API_KEY").map(String::as_str),
            Some("secret-value")
        );
    }

    #[tokio::test]
    async fn apply_registry_writes_provider_catalog_and_explicit_route() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let registry = ProviderRegistry {
            schema_version: REGISTRY_VERSION,
            providers: BTreeMap::from([(
                "gateway".to_string(),
                ManagedProvider {
                    name: "Company Gateway".to_string(),
                    base_url: "https://gateway.example.com/v1".to_string(),
                    env_key: "COMPANY_API_KEY".to_string(),
                    supports_websockets: false,
                    models: vec![default_model(
                        "company-fast".to_string(),
                        "vendor/model-v3".to_string(),
                    )],
                },
            )]),
        };

        apply_registry(home.path(), &registry)
            .await
            .expect("registry should apply");

        let routes: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.path().join(CATALOG_DIR).join(ROUTES_FILE))
                .expect("routes should be written"),
        )
        .expect("routes should be valid json");
        assert_eq!(routes["models"]["company-fast"]["provider"], "gateway");
        assert_eq!(
            routes["models"]["company-fast"]["api_model"],
            "vendor/model-v3"
        );
        let catalog: ModelsResponse = serde_json::from_str(
            &std::fs::read_to_string(home.path().join(CATALOG_DIR).join("gateway.json"))
                .expect("catalog should be written"),
        )
        .expect("catalog should be valid json");
        assert_eq!(catalog.models[0].slug, "vendor/model-v3");
        let config = std::fs::read_to_string(home.path().join("config.toml"))
            .expect("config should be written");
        assert!(config.contains("[model_providers.gateway]"));
        assert!(config.contains("env_key = \"COMPANY_API_KEY\""));
    }

    #[tokio::test]
    async fn apply_registry_removes_artifacts_for_deleted_managed_provider() {
        let home = tempfile::tempdir().expect("temp dir should be created");
        let mut registry = ProviderRegistry {
            schema_version: REGISTRY_VERSION,
            providers: BTreeMap::from([(
                "gateway".to_string(),
                ManagedProvider {
                    name: "Company Gateway".to_string(),
                    base_url: "https://gateway.example.com/v1".to_string(),
                    env_key: "COMPANY_API_KEY".to_string(),
                    supports_websockets: false,
                    models: vec![default_model(
                        "company-fast".to_string(),
                        "vendor/model-v3".to_string(),
                    )],
                },
            )]),
        };
        apply_registry(home.path(), &registry)
            .await
            .expect("initial registry should apply");

        registry.providers.clear();
        apply_registry(home.path(), &registry)
            .await
            .expect("empty registry should remove stale artifacts");

        assert!(!home.path().join(CATALOG_DIR).join("gateway.json").exists());
        let config = std::fs::read_to_string(home.path().join("config.toml"))
            .expect("config should be written");
        assert!(!config.contains("model_providers.gateway"));
        let applied = load_applied_providers(home.path()).expect("manifest should load");
        assert!(applied.providers.is_empty());
    }
}
