use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::client_gen::cli::CliGenerator;
use crate::client_gen::generator::{ClientGenerator, GeneratorConfig};
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
    match kind {
        FfiClientArtifactKind::Cli => CliGenerator.generate(&config),
        FfiClientArtifactKind::Python => PythonGenerator.generate(&config),
        FfiClientArtifactKind::TypeScript => TypeScriptGenerator.generate(&config),
    }
}
