use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::client_gen::cli::CliGenerator;
use crate::client_gen::generator::{artifact_map, write_artifacts, ClientGenerator, GeneratedArtifact, GeneratorConfig};
use crate::client_gen::python::PythonGenerator;
use crate::client_gen::typescript::TypeScriptGenerator;
use crate::Error;

use super::dto::FfiGeneratorConfig;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FfiClientArtifactKind {
    Cli,
    Python,
    TypeScript,
}

pub fn generate_client_artifacts(
    kind: FfiClientArtifactKind,
    config: FfiGeneratorConfig,
) -> Result<Vec<PathBuf>, Error> {
    let config = GeneratorConfig::from(config);
    let artifacts = render_client_artifacts_from_config(kind, &config)?;
    write_artifacts(&artifacts, &config.output_dir)
}

pub fn generate_client_artifact_files(
    kind: FfiClientArtifactKind,
    config: FfiGeneratorConfig,
) -> Result<BTreeMap<String, String>, Error> {
    let config = GeneratorConfig::from(config);
    let artifacts = render_client_artifacts_from_config(kind, &config)?;
    Ok(artifact_map(&artifacts))
}

pub fn render_client_artifacts(
    kind: FfiClientArtifactKind,
    config: FfiGeneratorConfig,
) -> Result<Vec<GeneratedArtifact>, Error> {
    let config = GeneratorConfig::from(config);
    render_client_artifacts_from_config(kind, &config)
}

fn render_client_artifacts_from_config(
    kind: FfiClientArtifactKind,
    config: &GeneratorConfig,
) -> Result<Vec<GeneratedArtifact>, Error> {
    match kind {
        FfiClientArtifactKind::Cli => CliGenerator.render(config),
        FfiClientArtifactKind::Python => PythonGenerator.render(config),
        FfiClientArtifactKind::TypeScript => TypeScriptGenerator.render(config),
    }
}
