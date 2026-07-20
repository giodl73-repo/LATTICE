#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use lattice_model::{
    Bond, BondKind, ClosedCut, ClosureReceipt, ContextPack, CutId, Grain, GrainId, GrainKind,
    PressPublicationFrame, PromptFrame,
};
use lattice_registry::{
    IngestOutcome, IngestReceipt, IngestRecord, SourcePointer, SOURCE_POINTER_LEGACY_CUSTODY_FIELD,
};

const GENERATION_FILE: &str = "generation";
const REGISTRY_FILE: &str = "source-pointers.tsv";
const WRITER_LOCK_FILE: &str = ".writer.lock";
const WORKSPACE_DIRS: [&str; 6] = ["registry", "store", "receipts", "cuts", "packs", "cache"];

/// Current `source-pointers.tsv` header. Extended from 9 to 13 columns by the
/// 2026-05-22 Live Search L06 follow-on so the persisted row carries the four
/// custody fields the L06 JSON envelope already advertises
/// (`refresh_status`, `refresh_check`, `custody_owner`, `custody_distributor`).
const SOURCE_POINTER_HEADER: &str = "source_id\towner_repo\twork_id\tfletch_registry_path\tfletch_id\tproof_ledger_path\tproof_record_path\trights_policy\trights_boundary\trefresh_status\trefresh_check\tcustody_owner\tcustody_distributor\n";

/// Legacy 9-column header, kept as a constant so the loader can accept old
/// workspaces written before the L06 follow-on without mis-classifying the
/// header row as a malformed pointer. New columns are filled with the
/// documented migration sentinel
/// ([`SOURCE_POINTER_LEGACY_CUSTODY_FIELD`]).
const SOURCE_POINTER_HEADER_LEGACY_V1: &str = "source_id\towner_repo\twork_id\tfletch_registry_path\tfletch_id\tproof_ledger_path\tproof_record_path\trights_policy\trights_boundary";

#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    cuts: BTreeMap<CutId, ClosedCut>,
    packs: BTreeMap<String, ContextPack>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_cut(&mut self, cut: ClosedCut) -> Option<ClosedCut> {
        self.cuts.insert(cut.id.clone(), cut)
    }

    pub fn get_cut(&self, id: &CutId) -> Option<&ClosedCut> {
        self.cuts.get(id)
    }

    pub fn cut_count(&self) -> usize {
        self.cuts.len()
    }

    pub fn materialize_pack_from_cut(
        &mut self,
        id: &CutId,
        profile_id: impl Into<String>,
    ) -> Option<&ContextPack> {
        let pack = ContextPack::from_closed_cut(self.get_cut(id)?, profile_id);
        let pack_id = pack.id.clone();
        self.packs.insert(pack_id.clone(), pack);
        self.packs.get(&pack_id)
    }

    pub fn get_pack(&self, id: &str) -> Option<&ContextPack> {
        self.packs.get(id)
    }

    pub fn pack_count(&self) -> usize {
        self.packs.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceStatus {
    pub path: PathBuf,
    pub generation: u64,
    pub directories: Vec<WorkspaceDirectoryStatus>,
}

impl WorkspaceStatus {
    pub fn is_complete(&self) -> bool {
        self.directories.iter().all(|directory| directory.present)
    }

    pub fn directory_count(&self) -> usize {
        self.directories.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDirectoryStatus {
    pub name: String,
    pub present: bool,
}

#[derive(Debug)]
pub enum WorkspaceError {
    MissingWorkspace {
        path: PathBuf,
    },
    DuplicateSourcePointer {
        source_id: String,
        receipt_path: PathBuf,
        generation: u64,
    },
    GenerationConflict {
        command: String,
        expected_generation: u64,
        current_generation: u64,
        receipt_path: PathBuf,
        generation: u64,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    InvalidGeneration {
        path: PathBuf,
        value: String,
    },
    InvalidRegistryRow {
        path: PathBuf,
        line_number: usize,
        message: String,
    },
    MissingSourcePointer {
        source_id: String,
    },
    InvalidSourcePointer {
        source_id: String,
        errors: Vec<String>,
        receipt_path: PathBuf,
        generation: u64,
    },
    InvalidViewFilter {
        filter: String,
        message: String,
    },
    MissingIngestRecords,
    MissingView {
        view_id: String,
    },
    MissingCandidateCut {
        cut_id: String,
    },
    MissingClosedCut {
        cut_id: String,
    },
}

impl WorkspaceError {
    fn io(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingWorkspace { path } => {
                write!(formatter, "workspace does not exist: {}", path.display())
            }
            Self::DuplicateSourcePointer {
                source_id,
                receipt_path,
                generation,
            } => write!(
                formatter,
                "source pointer {source_id} already exists; failure receipt {} committed at generation {generation}",
                receipt_path.display()
            ),
            Self::GenerationConflict {
                command,
                expected_generation,
                current_generation,
                receipt_path,
                generation,
            } => write!(
                formatter,
                "{command} saw workspace generation {expected_generation}, but current generation is {current_generation}; conflict receipt {} committed at generation {generation}",
                receipt_path.display()
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(
                formatter,
                "workspace {action} failed for {}: {source}",
                path.display()
            ),
            Self::InvalidGeneration { path, value } => write!(
                formatter,
                "workspace generation file {} contains invalid value {:?}",
                path.display(),
                value
            ),
            Self::InvalidRegistryRow {
                path,
                line_number,
                message,
            } => write!(
                formatter,
                "registry row {line_number} in {} is invalid: {message}",
                path.display()
            ),
            Self::MissingSourcePointer { source_id } => {
                write!(formatter, "source pointer is not registered: {source_id}")
            }
            Self::InvalidSourcePointer {
                source_id,
                errors,
                receipt_path,
                generation,
            } => write!(
                formatter,
                "source pointer {source_id} failed validation ({}) ; failure receipt {} committed at generation {generation}",
                errors.join(", "),
                receipt_path.display()
            ),
            Self::InvalidViewFilter { filter, message } => {
                write!(formatter, "view filter {:?} is invalid: {message}", filter)
            }
            Self::MissingIngestRecords => write!(
                formatter,
                "workspace has no file-backed ingest records; run ingest before model/view/cut"
            ),
            Self::MissingView { view_id } => {
                write!(formatter, "workspace view does not exist: {view_id}")
            }
            Self::MissingCandidateCut { cut_id } => {
                write!(formatter, "workspace candidate cut does not exist: {cut_id}")
            }
            Self::MissingClosedCut { cut_id } => {
                write!(formatter, "workspace closed cut does not exist: {cut_id}")
            }
        }
    }
}

impl std::error::Error for WorkspaceError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryAddResult {
    pub source_id: String,
    pub generation: u64,
    pub registry_path: PathBuf,
    pub receipt_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryList {
    pub path: PathBuf,
    pub generation: u64,
    pub pointers: Vec<SourcePointer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestResult {
    pub source_id: String,
    pub generation: u64,
    pub outcome: String,
    pub record_path: PathBuf,
    pub receipt_path: PathBuf,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelBuildResult {
    pub generation: u64,
    pub model_path: PathBuf,
    pub receipt_path: PathBuf,
    pub ingest_record_count: usize,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewBuildResult {
    pub view_id: String,
    pub generation: u64,
    pub view_path: PathBuf,
    pub receipt_path: PathBuf,
    pub filter: String,
    pub predicate_count: usize,
    pub source_count: usize,
    pub grain_count: usize,
    pub bond_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateCutResult {
    pub cut_id: String,
    pub view_id: String,
    pub generation: u64,
    pub cut_path: PathBuf,
    pub receipt_path: PathBuf,
    pub token_budget: u64,
    pub estimated_tokens: u64,
    pub source_count: usize,
    pub grain_count: usize,
    pub bond_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCloseResult {
    pub cut_id: String,
    pub generation: u64,
    pub closed_path: PathBuf,
    pub receipt_path: PathBuf,
    pub cut_hash: String,
    pub receipt_hash: String,
    pub closure_policy: String,
    pub rights_policy: String,
    pub budget_status: String,
    pub token_budget: u64,
    pub estimated_tokens: u64,
    pub source_count: usize,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub frontier_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspacePackResult {
    pub cut_id: String,
    pub generation: u64,
    pub output_path: PathBuf,
    pub receipt_path: PathBuf,
    pub cache_manifest_path: PathBuf,
    pub pack_id: String,
    pub profile_id: String,
    pub cut_hash: String,
    pub receipt_hash: String,
    pub cache_prefix: String,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub closure_policy: String,
    pub rights_policy: String,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspacePromptResult {
    pub cut_id: String,
    pub generation: u64,
    pub output_path: PathBuf,
    pub receipt_path: PathBuf,
    pub cache_manifest_path: PathBuf,
    pub frame_version: String,
    pub pack_id: String,
    pub profile_id: String,
    pub contract: String,
    pub cut_hash: String,
    pub receipt_hash: String,
    pub cache_prefix: String,
    pub grain_count: usize,
    pub receipt_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspacePressFrameResult {
    pub cut_id: String,
    pub generation: u64,
    pub output_path: PathBuf,
    pub receipt_path: PathBuf,
    pub cache_manifest_path: PathBuf,
    pub frame_version: String,
    pub pack_id: String,
    pub target_family: String,
    pub handoff_contract: String,
    pub cut_hash: String,
    pub receipt_hash: String,
    pub cache_prefix: String,
    pub receipt_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceIndexResult {
    pub generation: u64,
    pub index_path: PathBuf,
    pub receipt_path: PathBuf,
    pub cache_manifest_path: PathBuf,
    pub artifact_profile: String,
    pub artifact_format: String,
    pub ingest_record_count: usize,
    pub source_count: usize,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub index_entry_count: usize,
    pub elapsed_millis: u128,
    pub output_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactProfile {
    Audit,
    Compact,
    Pebble,
}

impl ArtifactProfile {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "audit" => Some(Self::Audit),
            "compact" => Some(Self::Compact),
            "pebble" | "pebble-compact" => Some(Self::Pebble),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audit => "audit",
            Self::Compact => "compact",
            Self::Pebble => "pebble",
        }
    }

    fn format(self) -> &'static str {
        match self {
            Self::Audit => "lattice.source-corpus-index.v1",
            Self::Compact => "lattice.source-corpus-index.compact.v1",
            Self::Pebble => "lattice.pebble-source-corpus-index.v1",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IngestRecordSummary {
    source_id: String,
    owner_repo: String,
    rights_policy: String,
    grain_count: usize,
    bond_count: usize,
    receipt_count: usize,
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkspaceViewRecord {
    view_id: String,
    filter: String,
    source_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkspaceCandidateCutRecord {
    cut_id: String,
    view_id: String,
    source_ids: Vec<String>,
    token_budget: u64,
    estimated_tokens: u64,
}

impl Workspace {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init(path: impl Into<PathBuf>) -> Result<WorkspaceStatus, WorkspaceError> {
        let workspace = Self::at(path);
        workspace.initialize()
    }

    pub fn status(path: impl Into<PathBuf>) -> Result<WorkspaceStatus, WorkspaceError> {
        Self::at(path).read_status()
    }

    pub fn add_source_pointer(
        path: impl Into<PathBuf>,
        pointer: SourcePointer,
    ) -> Result<RegistryAddResult, WorkspaceError> {
        Self::at(path).add_pointer(pointer)
    }

    pub fn list_source_pointers(path: impl Into<PathBuf>) -> Result<RegistryList, WorkspaceError> {
        Self::at(path).list_pointers()
    }

    pub fn ingest_source_pointer(
        path: impl Into<PathBuf>,
        source_id: &str,
    ) -> Result<IngestResult, WorkspaceError> {
        Self::at(path).ingest_source(source_id)
    }

    pub fn build_model(path: impl Into<PathBuf>) -> Result<ModelBuildResult, WorkspaceError> {
        Self::at(path).build_workspace_model()
    }

    pub fn build_view(
        path: impl Into<PathBuf>,
        view_id: &str,
        filter: &str,
    ) -> Result<ViewBuildResult, WorkspaceError> {
        Self::at(path).build_workspace_view(view_id, filter)
    }

    pub fn build_candidate_cut(
        path: impl Into<PathBuf>,
        view_id: &str,
        token_budget: u64,
    ) -> Result<CandidateCutResult, WorkspaceError> {
        Self::at(path).build_workspace_cut(view_id, token_budget)
    }

    pub fn close_candidate_cut(
        path: impl Into<PathBuf>,
        cut_id: &str,
    ) -> Result<WorkspaceCloseResult, WorkspaceError> {
        Self::at(path).close_workspace_cut(cut_id)
    }

    pub fn read_closed_cut(
        path: impl Into<PathBuf>,
        cut_id: &str,
    ) -> Result<ClosedCut, WorkspaceError> {
        Self::at(path).read_workspace_closed_cut(cut_id)
    }

    pub fn write_pack(
        path: impl Into<PathBuf>,
        cut_id: &str,
    ) -> Result<WorkspacePackResult, WorkspaceError> {
        Self::at(path).write_workspace_pack(cut_id, "workspace-pack")
    }

    pub fn write_pack_with_profile(
        path: impl Into<PathBuf>,
        cut_id: &str,
        profile_id: &str,
    ) -> Result<WorkspacePackResult, WorkspaceError> {
        Self::at(path).write_workspace_pack(cut_id, profile_id)
    }

    pub fn write_prompt_frame(
        path: impl Into<PathBuf>,
        cut_id: &str,
    ) -> Result<WorkspacePromptResult, WorkspaceError> {
        Self::at(path).write_workspace_prompt(cut_id, "workspace-prompt")
    }

    pub fn write_prompt_frame_with_profile(
        path: impl Into<PathBuf>,
        cut_id: &str,
        profile_id: &str,
    ) -> Result<WorkspacePromptResult, WorkspaceError> {
        Self::at(path).write_workspace_prompt(cut_id, profile_id)
    }

    pub fn write_press_frame(
        path: impl Into<PathBuf>,
        cut_id: &str,
    ) -> Result<WorkspacePressFrameResult, WorkspaceError> {
        Self::at(path).write_workspace_press_frame(cut_id)
    }

    pub fn build_source_corpus_index(
        path: impl Into<PathBuf>,
    ) -> Result<WorkspaceIndexResult, WorkspaceError> {
        Self::at(path).build_workspace_source_corpus_index(ArtifactProfile::Audit)
    }

    pub fn build_source_corpus_index_with_artifact_profile(
        path: impl Into<PathBuf>,
        artifact_profile: ArtifactProfile,
    ) -> Result<WorkspaceIndexResult, WorkspaceError> {
        Self::at(path).build_workspace_source_corpus_index(artifact_profile)
    }

    fn initialize(&self) -> Result<WorkspaceStatus, WorkspaceError> {
        fs::create_dir_all(&self.path)
            .map_err(|error| WorkspaceError::io("create root directory", &self.path, error))?;

        let _lock = WorkspaceWriterLock::acquire(&self.path)?;

        for directory in WORKSPACE_DIRS {
            let path = self.path.join(directory);
            fs::create_dir_all(&path).map_err(|error| {
                WorkspaceError::io("create workspace directory", path.clone(), error)
            })?;
        }

        let current_generation = self.read_generation_or_zero()?;
        let next_generation = current_generation + 1;
        self.write_init_receipt(next_generation)?;
        self.write_generation(next_generation)?;
        self.read_status()
    }

    fn read_status(&self) -> Result<WorkspaceStatus, WorkspaceError> {
        if !self.path.is_dir() {
            return Err(WorkspaceError::MissingWorkspace {
                path: self.path.clone(),
            });
        }

        let generation = self.read_generation_or_zero()?;
        let directories = WORKSPACE_DIRS
            .iter()
            .map(|name| WorkspaceDirectoryStatus {
                name: (*name).to_string(),
                present: self.path.join(name).is_dir(),
            })
            .collect();

        Ok(WorkspaceStatus {
            path: self.path.clone(),
            generation,
            directories,
        })
    }

    fn read_generation_or_zero(&self) -> Result<u64, WorkspaceError> {
        let path = self.path.join(GENERATION_FILE);
        if !path.exists() {
            return Ok(0);
        }

        let value = fs::read_to_string(&path)
            .map_err(|error| WorkspaceError::io("read generation", path.clone(), error))?;
        value
            .trim()
            .parse()
            .map_err(|_| WorkspaceError::InvalidGeneration {
                path,
                value: value.trim().to_string(),
            })
    }

    fn write_generation(&self, generation: u64) -> Result<(), WorkspaceError> {
        let path = self.path.join(GENERATION_FILE);
        fs::write(&path, format!("{generation}\n"))
            .map_err(|error| WorkspaceError::io("write generation", path, error))
    }

    fn write_init_receipt(&self, generation: u64) -> Result<(), WorkspaceError> {
        let path = self
            .path
            .join("receipts")
            .join(format!("workspace-init-{generation:010}.json"));
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"workspace-init\",",
                "\"generation\":{},",
                "\"status\":\"committed\"",
                "}}\n"
            ),
            generation
        );
        fs::write(&path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn add_pointer(&self, pointer: SourcePointer) -> Result<RegistryAddResult, WorkspaceError> {
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("registry-add", starting_generation)?;
        let generation = starting_generation + 1;
        let receipt_path = self.registry_receipt_path(&pointer.source_id, generation);

        let validation = pointer.validate();
        if !validation.is_ok() {
            let errors = validation
                .errors
                .iter()
                .map(|error| format!("{:?}: {}", error.field, error.message))
                .collect::<Vec<_>>();
            self.write_registry_receipt(
                generation,
                "failed",
                &pointer.source_id,
                &format!("source pointer validation failed: {}", errors.join("; ")),
                &receipt_path,
            )?;
            self.write_generation(generation)?;
            return Err(WorkspaceError::InvalidSourcePointer {
                source_id: pointer.source_id,
                errors,
                receipt_path,
                generation,
            });
        }

        if self
            .read_source_pointers()?
            .iter()
            .any(|existing| existing.source_id == pointer.source_id)
        {
            self.write_registry_receipt(
                generation,
                "failed",
                &pointer.source_id,
                "source pointer already exists",
                &receipt_path,
            )?;
            self.write_generation(generation)?;
            return Err(WorkspaceError::DuplicateSourcePointer {
                source_id: pointer.source_id,
                receipt_path,
                generation,
            });
        }

        let registry_path = self.registry_path();
        self.append_source_pointer(&pointer)?;
        self.write_registry_receipt(
            generation,
            "committed",
            &pointer.source_id,
            "source pointer registered",
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(RegistryAddResult {
            source_id: pointer.source_id,
            generation,
            registry_path,
            receipt_path,
        })
    }

    fn list_pointers(&self) -> Result<RegistryList, WorkspaceError> {
        let status = self.read_status()?;
        Ok(RegistryList {
            path: self.registry_path(),
            generation: status.generation,
            pointers: self.read_source_pointers()?,
        })
    }

    fn registry_path(&self) -> PathBuf {
        self.path.join("registry").join(REGISTRY_FILE)
    }

    fn registry_receipt_path(&self, source_id: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "registry-add-{}-{generation:010}.json",
            safe_file_stem(source_id)
        ))
    }

    fn append_source_pointer(&self, pointer: &SourcePointer) -> Result<(), WorkspaceError> {
        let registry_path = self.registry_path();
        let needs_header = !registry_path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&registry_path)
            .map_err(|error| WorkspaceError::io("open registry", registry_path.clone(), error))?;
        if needs_header {
            file.write_all(SOURCE_POINTER_HEADER.as_bytes())
                .map_err(|error| {
                    WorkspaceError::io("write registry header", registry_path.clone(), error)
                })?;
        }
        file.write_all(source_pointer_row(pointer).as_bytes())
            .map_err(|error| WorkspaceError::io("write registry row", registry_path.clone(), error))
    }

    fn read_source_pointers(&self) -> Result<Vec<SourcePointer>, WorkspaceError> {
        let path = self.registry_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| WorkspaceError::io("read registry", path.clone(), error))?;
        let mut pointers = Vec::new();
        for (index, line) in contents.lines().enumerate() {
            let line_number = index + 1;
            if index == 0
                && (line == SOURCE_POINTER_HEADER.trim_end()
                    || line == SOURCE_POINTER_HEADER_LEGACY_V1)
            {
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            pointers.push(source_pointer_from_row(&path, line_number, line)?);
        }
        Ok(pointers)
    }

    fn write_registry_receipt(
        &self,
        generation: u64,
        status: &str,
        source_id: &str,
        note: &str,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"registry-add\",",
                "\"generation\":{},",
                "\"status\":\"{}\",",
                "\"source_id\":\"{}\",",
                "\"note\":\"{}\"",
                "}}\n"
            ),
            generation,
            json_escape(status),
            json_escape(source_id),
            json_escape(note)
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn ingest_source(&self, source_id: &str) -> Result<IngestResult, WorkspaceError> {
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("ingest", starting_generation)?;
        let generation = starting_generation + 1;
        let pointer = self
            .read_source_pointers()?
            .into_iter()
            .find(|pointer| pointer.source_id == source_id)
            .ok_or_else(|| WorkspaceError::MissingSourcePointer {
                source_id: source_id.to_string(),
            })?;

        let record = IngestRecord::tiny_fixture(&pointer);
        let record_path = self.ingest_record_path(source_id);
        let receipt_path = self.ingest_receipt_path(source_id, generation);
        self.write_ingest_record(&record, &record_path)?;
        self.write_ingest_receipt(generation, &record, &receipt_path)?;
        self.write_generation(generation)?;

        let grain_count = record.grain_count();
        let bond_count = record.bond_count();
        let receipt_count = record.receipts.len();
        Ok(IngestResult {
            source_id: record.source_id,
            generation,
            outcome: record.outcome.as_str().to_string(),
            record_path,
            receipt_path,
            grain_count,
            bond_count,
            receipt_count,
        })
    }

    fn ingest_record_path(&self, source_id: &str) -> PathBuf {
        self.path
            .join("store")
            .join("ingest-records")
            .join(format!("{}.json", safe_file_stem(source_id)))
    }

    fn ingest_receipt_path(&self, source_id: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "ingest-{}-{generation:010}.json",
            safe_file_stem(source_id)
        ))
    }

    fn write_ingest_record(
        &self,
        record: &IngestRecord,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                WorkspaceError::io("create ingest record directory", parent, error)
            })?;
        }
        fs::write(path, ingest_record_json(record))
            .map_err(|error| WorkspaceError::io("write ingest record", path, error))
    }

    fn write_ingest_receipt(
        &self,
        generation: u64,
        record: &IngestRecord,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"ingest\",",
                "\"generation\":{},",
                "\"status\":\"committed\",",
                "\"source_id\":\"{}\",",
                "\"outcome\":\"{}\",",
                "\"grain_count\":{},",
                "\"bond_count\":{},",
                "\"receipt_count\":{}",
                "}}\n"
            ),
            generation,
            json_escape(&record.source_id),
            record.outcome.as_str(),
            record.grain_count(),
            record.bond_count(),
            record.receipts.len()
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn build_workspace_model(&self) -> Result<ModelBuildResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("model-build", starting_generation)?;
        let summaries = self.read_ingest_record_summaries()?;
        if summaries.is_empty() {
            return Err(WorkspaceError::MissingIngestRecords);
        }

        let generation = starting_generation + 1;
        let model_path = self.path.join("store").join("model.json");
        let receipt_path = self.model_receipt_path(generation);
        let ingest_record_count = summaries.len();
        let grain_count = summaries.iter().map(|summary| summary.grain_count).sum();
        let bond_count = summaries.iter().map(|summary| summary.bond_count).sum();
        let receipt_count = summaries.iter().map(|summary| summary.receipt_count).sum();
        let body = model_json(&summaries);
        let output_bytes = body.len() as u64;
        fs::write(&model_path, body)
            .map_err(|error| WorkspaceError::io("write model", model_path.clone(), error))?;
        self.write_model_receipt(
            generation,
            ingest_record_count,
            grain_count,
            bond_count,
            receipt_count,
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(ModelBuildResult {
            generation,
            model_path,
            receipt_path,
            ingest_record_count,
            grain_count,
            bond_count,
            receipt_count,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn build_workspace_view(
        &self,
        view_id: &str,
        filter: &str,
    ) -> Result<ViewBuildResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("view-build", starting_generation)?;
        let summaries = self.read_ingest_record_summaries()?;
        if summaries.is_empty() {
            return Err(WorkspaceError::MissingIngestRecords);
        }
        let predicates = parse_filter(filter)?;
        let selected = summaries
            .iter()
            .filter(|summary| {
                predicates
                    .iter()
                    .all(|predicate| predicate.matches(summary))
            })
            .cloned()
            .collect::<Vec<_>>();

        let generation = starting_generation + 1;
        let view_path = self.view_path(view_id);
        let receipt_path = self.view_receipt_path(view_id, generation);
        if let Some(parent) = view_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| WorkspaceError::io("create view directory", parent, error))?;
        }
        let source_ids = selected
            .iter()
            .map(|summary| summary.source_id.clone())
            .collect::<Vec<_>>();
        let grain_count = selected.iter().map(|summary| summary.grain_count).sum();
        let bond_count = selected.iter().map(|summary| summary.bond_count).sum();
        let body = view_json(
            view_id,
            filter,
            predicates.len(),
            &source_ids,
            grain_count,
            bond_count,
        );
        let output_bytes = body.len() as u64;
        fs::write(&view_path, body)
            .map_err(|error| WorkspaceError::io("write view", view_path.clone(), error))?;
        self.write_view_receipt(
            generation,
            view_id,
            filter,
            source_ids.len(),
            grain_count,
            bond_count,
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(ViewBuildResult {
            view_id: view_id.to_string(),
            generation,
            view_path,
            receipt_path,
            filter: filter.to_string(),
            predicate_count: predicates.len(),
            source_count: source_ids.len(),
            grain_count,
            bond_count,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn build_workspace_cut(
        &self,
        view_id: &str,
        token_budget: u64,
    ) -> Result<CandidateCutResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("cut-build", starting_generation)?;
        let view = self.read_view_record(view_id)?;
        let summaries = self.read_ingest_record_summaries()?;
        let selected = summaries
            .into_iter()
            .filter(|summary| {
                view.source_ids
                    .iter()
                    .any(|source_id| source_id == &summary.source_id)
            })
            .collect::<Vec<_>>();

        let generation = starting_generation + 1;
        let cut_id = view_id.to_string();
        let cut_path = self.candidate_cut_path(&cut_id);
        let receipt_path = self.cut_receipt_path(&cut_id, generation);
        if let Some(parent) = cut_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                WorkspaceError::io("create candidate cut directory", parent, error)
            })?;
        }
        let source_ids = selected
            .iter()
            .map(|summary| summary.source_id.clone())
            .collect::<Vec<_>>();
        let grain_count = selected.iter().map(|summary| summary.grain_count).sum();
        let bond_count = selected.iter().map(|summary| summary.bond_count).sum();
        let estimated_tokens = estimate_tokens(grain_count, bond_count);
        let body = candidate_cut_json(
            &cut_id,
            view_id,
            &view.filter,
            token_budget,
            estimated_tokens,
            &source_ids,
            grain_count,
            bond_count,
        );
        let output_bytes = body.len() as u64;
        fs::write(&cut_path, body)
            .map_err(|error| WorkspaceError::io("write candidate cut", cut_path.clone(), error))?;
        self.write_cut_receipt(
            generation,
            &cut_id,
            view_id,
            source_ids.len(),
            grain_count,
            bond_count,
            token_budget,
            estimated_tokens,
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(CandidateCutResult {
            cut_id,
            view_id: view_id.to_string(),
            generation,
            cut_path,
            receipt_path,
            token_budget,
            estimated_tokens,
            source_count: source_ids.len(),
            grain_count,
            bond_count,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn close_workspace_cut(&self, cut_id: &str) -> Result<WorkspaceCloseResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("close", starting_generation)?;
        let candidate = self.read_candidate_cut_record(cut_id)?;
        let records = self.read_ingest_records_for_sources(&candidate.source_ids)?;
        let generation = starting_generation + 1;
        let closed_path = self.closed_cut_path(cut_id);
        let receipt_path = self.close_receipt_path(cut_id, generation);
        if let Some(parent) = closed_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                WorkspaceError::io("create closed cut directory", parent, error)
            })?;
        }

        let mut closed = ClosedCut::new(CutId::new(cut_id));
        for record in &records {
            for grain in &record.grains {
                closed.grains.insert(grain.id.clone());
                closed.grain_records.push(grain.clone());
            }
        }
        for record in &records {
            for bond in &record.bonds {
                if closed.contains_bond_endpoints(bond) {
                    closed.bonds.insert(bond.clone());
                }
            }
        }
        for record in &records {
            closed.closure_receipts.push(ClosureReceipt::new(
                "lattice.workspace.ingest-receipt",
                format!(
                    "included {} ingest receipt(s) for {}",
                    record.receipts.len(),
                    record.source_id
                ),
            ));
        }
        closed.closure_receipts.push(ClosureReceipt::new(
            "lattice.workspace.close-v1",
            format!(
                "closed candidate cut {} from view {} with {} source record(s)",
                candidate.cut_id,
                candidate.view_id,
                records.len()
            ),
        ));
        let rights_policy = rights_policy_for_records(&records);
        closed = closed.with_policy("workspace-close-v1", rights_policy.clone());

        let cut_hash = closed.stable_hash();
        let receipt_hash = closed.receipt_hash();
        let body = closed_cut_json(
            &closed,
            &candidate,
            &cut_hash,
            &receipt_hash,
            "within_budget",
            0,
        );
        let output_bytes = body.len() as u64;
        fs::write(&closed_path, body)
            .map_err(|error| WorkspaceError::io("write closed cut", closed_path.clone(), error))?;
        self.write_close_receipt(
            generation,
            cut_id,
            &cut_hash,
            &receipt_hash,
            "within_budget",
            closed.grains.len(),
            closed.bonds.len(),
            closed.closure_receipts.len(),
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(WorkspaceCloseResult {
            cut_id: cut_id.to_string(),
            generation,
            closed_path,
            receipt_path,
            cut_hash,
            receipt_hash,
            closure_policy: "workspace-close-v1".to_string(),
            rights_policy,
            budget_status: "within_budget".to_string(),
            token_budget: candidate.token_budget,
            estimated_tokens: candidate.estimated_tokens,
            source_count: candidate.source_ids.len(),
            grain_count: closed.grains.len(),
            bond_count: closed.bonds.len(),
            receipt_count: closed.closure_receipts.len(),
            frontier_count: 0,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn write_workspace_pack(
        &self,
        cut_id: &str,
        profile_id: &str,
    ) -> Result<WorkspacePackResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("pack", starting_generation)?;
        let cut = self.read_workspace_closed_cut(cut_id)?;
        let generation = starting_generation + 1;
        let pack = ContextPack::from_closed_cut(&cut, profile_id);
        let output_path = self.pack_path(cut_id);
        let receipt_path = self.handoff_receipt_path("pack", cut_id, generation);
        let cache_manifest_path = self.cache_manifest_path("pack", cut_id);
        self.ensure_parent(&output_path, "create pack directory")?;
        self.ensure_parent(&cache_manifest_path, "create cache directory")?;
        let cut_hash = pack.cut_hash.clone();
        let receipt_hash = pack.receipt_hash.clone();
        let cache_prefix = pack.cache_prefix.clone();
        let body = pack_metadata_json(&pack);
        let output_bytes = body.len() as u64;
        fs::write(&output_path, body).map_err(|error| {
            WorkspaceError::io("write pack metadata", output_path.clone(), error)
        })?;
        self.write_cache_manifest(
            generation,
            "pack",
            cut_id,
            &pack.id,
            &cut_hash,
            &receipt_hash,
            &cache_prefix,
            &output_path,
            &receipt_path,
            &cache_manifest_path,
        )?;
        self.write_handoff_receipt(
            generation,
            "pack",
            cut_id,
            &pack.id,
            &cut_hash,
            &receipt_hash,
            &cache_prefix,
            &cache_manifest_path,
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(WorkspacePackResult {
            cut_id: cut_id.to_string(),
            generation,
            output_path,
            receipt_path,
            cache_manifest_path,
            pack_id: pack.id,
            profile_id: pack.profile_id,
            cut_hash,
            receipt_hash,
            cache_prefix,
            grain_count: pack.grain_count,
            bond_count: pack.bond_count,
            receipt_count: pack.receipt_count,
            closure_policy: pack.closure_policy.unwrap_or_else(|| "unknown".to_string()),
            rights_policy: pack.rights_policy.unwrap_or_else(|| "unknown".to_string()),
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn write_workspace_prompt(
        &self,
        cut_id: &str,
        profile_id: &str,
    ) -> Result<WorkspacePromptResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("prompt", starting_generation)?;
        let cut = self.read_workspace_closed_cut(cut_id)?;
        let generation = starting_generation + 1;
        let prompt = PromptFrame::from_closed_cut(&cut, profile_id);
        let output_path = self.prompt_path(cut_id);
        let receipt_path = self.handoff_receipt_path("prompt", cut_id, generation);
        let cache_manifest_path = self.cache_manifest_path("prompt", cut_id);
        self.ensure_parent(&output_path, "create prompt directory")?;
        self.ensure_parent(&cache_manifest_path, "create cache directory")?;
        let cut_hash = prompt.pack.cut_hash.clone();
        let receipt_hash = prompt.pack.receipt_hash.clone();
        let cache_prefix = prompt.pack.cache_prefix.clone();
        let body = prompt_frame_json(&prompt);
        let output_bytes = body.len() as u64;
        fs::write(&output_path, body).map_err(|error| {
            WorkspaceError::io("write prompt metadata", output_path.clone(), error)
        })?;
        self.write_cache_manifest(
            generation,
            "prompt",
            cut_id,
            &prompt.pack.id,
            &cut_hash,
            &receipt_hash,
            &cache_prefix,
            &output_path,
            &receipt_path,
            &cache_manifest_path,
        )?;
        self.write_handoff_receipt(
            generation,
            "prompt",
            cut_id,
            &prompt.pack.id,
            &cut_hash,
            &receipt_hash,
            &cache_prefix,
            &cache_manifest_path,
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(WorkspacePromptResult {
            cut_id: cut_id.to_string(),
            generation,
            output_path,
            receipt_path,
            cache_manifest_path,
            frame_version: prompt.frame_version,
            pack_id: prompt.pack.id,
            profile_id: prompt.pack.profile_id,
            contract: prompt.contract,
            cut_hash,
            receipt_hash,
            cache_prefix,
            grain_count: prompt.grain_ids.len(),
            receipt_count: prompt.receipt_count,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn write_workspace_press_frame(
        &self,
        cut_id: &str,
    ) -> Result<WorkspacePressFrameResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("export press-frame", starting_generation)?;
        let cut = self.read_workspace_closed_cut(cut_id)?;
        let generation = starting_generation + 1;
        let frame = PressPublicationFrame::from_closed_cut(&cut, "workspace-press", "press-frame");
        let output_path = self.press_frame_path(cut_id);
        let receipt_path = self.handoff_receipt_path("export-press-frame", cut_id, generation);
        let cache_manifest_path = self.cache_manifest_path("export-press-frame", cut_id);
        self.ensure_parent(&output_path, "create press frame directory")?;
        self.ensure_parent(&cache_manifest_path, "create cache directory")?;
        let cut_hash = frame.pack.cut_hash.clone();
        let receipt_hash = frame.pack.receipt_hash.clone();
        let cache_prefix = frame.pack.cache_prefix.clone();
        let body = press_frame_metadata_json(&frame);
        let output_bytes = body.len() as u64;
        fs::write(&output_path, body).map_err(|error| {
            WorkspaceError::io("write press frame metadata", output_path.clone(), error)
        })?;
        self.write_cache_manifest(
            generation,
            "export-press-frame",
            cut_id,
            &frame.pack.id,
            &cut_hash,
            &receipt_hash,
            &cache_prefix,
            &output_path,
            &receipt_path,
            &cache_manifest_path,
        )?;
        self.write_handoff_receipt(
            generation,
            "export-press-frame",
            cut_id,
            &frame.pack.id,
            &cut_hash,
            &receipt_hash,
            &cache_prefix,
            &cache_manifest_path,
            &receipt_path,
        )?;
        self.write_generation(generation)?;

        Ok(WorkspacePressFrameResult {
            cut_id: cut_id.to_string(),
            generation,
            output_path,
            receipt_path,
            cache_manifest_path,
            frame_version: frame.frame_version,
            pack_id: frame.pack.id,
            target_family: frame.target_family,
            handoff_contract: frame.handoff_contract,
            cut_hash,
            receipt_hash,
            cache_prefix,
            receipt_count: frame.receipt_count,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn build_workspace_source_corpus_index(
        &self,
        artifact_profile: ArtifactProfile,
    ) -> Result<WorkspaceIndexResult, WorkspaceError> {
        let started = Instant::now();
        let starting_generation = self.read_status()?.generation;
        let _lock = WorkspaceWriterLock::acquire(&self.path)?;
        self.verify_transaction_generation("build-indexes", starting_generation)?;
        let summaries = self.read_ingest_record_summaries()?;
        if summaries.is_empty() {
            return Err(WorkspaceError::MissingIngestRecords);
        }

        let generation = starting_generation + 1;
        let index_path = source_corpus_index_path(&self.path, artifact_profile);
        let cache_manifest_path = self
            .path
            .join("cache")
            .join("source-corpus-index-manifest.json");
        let receipt_path = self
            .path
            .join("receipts")
            .join(format!("build-indexes-{generation:010}.json"));
        let ingest_record_count = summaries.len();
        let source_count = summaries.len();
        let grain_count = summaries.iter().map(|summary| summary.grain_count).sum();
        let bond_count = summaries.iter().map(|summary| summary.bond_count).sum();
        let receipt_count = summaries.iter().map(|summary| summary.receipt_count).sum();
        let index_entry_count = source_count + grain_count + bond_count;
        let body = source_corpus_index_artifact(
            generation,
            &summaries,
            index_entry_count,
            artifact_profile,
        );
        let output_bytes = body.len() as u64;
        fs::write(&index_path, body).map_err(|error| {
            WorkspaceError::io("write source-corpus index", index_path.clone(), error)
        })?;
        fs::write(
            &cache_manifest_path,
            source_corpus_index_manifest_json(
                generation,
                &index_path,
                &receipt_path,
                artifact_profile,
                ingest_record_count,
                grain_count,
                bond_count,
                receipt_count,
                index_entry_count,
            ),
        )
        .map_err(|error| {
            WorkspaceError::io(
                "write source-corpus index manifest",
                cache_manifest_path.clone(),
                error,
            )
        })?;
        fs::write(
            &receipt_path,
            source_corpus_index_receipt_json(
                generation,
                ingest_record_count,
                artifact_profile,
                grain_count,
                bond_count,
                receipt_count,
                index_entry_count,
            ),
        )
        .map_err(|error| WorkspaceError::io("write receipt", receipt_path.clone(), error))?;
        self.write_generation(generation)?;

        Ok(WorkspaceIndexResult {
            generation,
            index_path,
            receipt_path,
            cache_manifest_path,
            artifact_profile: artifact_profile.as_str().to_string(),
            artifact_format: artifact_profile.format().to_string(),
            ingest_record_count,
            source_count,
            grain_count,
            bond_count,
            receipt_count,
            index_entry_count,
            elapsed_millis: started.elapsed().as_millis(),
            output_bytes,
        })
    }

    fn verify_transaction_generation(
        &self,
        command: &str,
        expected_generation: u64,
    ) -> Result<(), WorkspaceError> {
        let current_generation = self.read_generation_or_zero()?;
        if current_generation == expected_generation {
            return Ok(());
        }

        let conflict_generation = current_generation + 1;
        let receipt_path = self.generation_conflict_receipt_path(command, conflict_generation);
        self.write_generation_conflict_receipt(
            command,
            expected_generation,
            current_generation,
            conflict_generation,
            &receipt_path,
        )?;
        self.write_generation(conflict_generation)?;
        Err(WorkspaceError::GenerationConflict {
            command: command.to_string(),
            expected_generation,
            current_generation,
            receipt_path,
            generation: conflict_generation,
        })
    }

    fn generation_conflict_receipt_path(&self, command: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "generation-conflict-{}-{generation:010}.json",
            safe_file_stem(command)
        ))
    }

    fn write_generation_conflict_receipt(
        &self,
        command: &str,
        expected_generation: u64,
        current_generation: u64,
        conflict_generation: u64,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"generation-conflict\",",
                "\"command\":\"{}\",",
                "\"generation\":{},",
                "\"status\":\"failed\",",
                "\"expected_generation\":{},",
                "\"current_generation\":{}",
                "}}\n"
            ),
            json_escape(command),
            conflict_generation,
            expected_generation,
            current_generation
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn ingest_records_dir(&self) -> PathBuf {
        self.path.join("store").join("ingest-records")
    }

    fn read_ingest_record_summaries(&self) -> Result<Vec<IngestRecordSummary>, WorkspaceError> {
        let dir = self.ingest_records_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut summaries = Vec::new();
        for entry in fs::read_dir(&dir)
            .map_err(|error| WorkspaceError::io("read ingest records", dir.clone(), error))?
        {
            let entry = entry.map_err(|error| {
                WorkspaceError::io("read ingest record entry", dir.clone(), error)
            })?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let contents = fs::read_to_string(&path)
                .map_err(|error| WorkspaceError::io("read ingest record", path.clone(), error))?;
            summaries.push(ingest_summary_from_json(&path, &contents)?);
        }
        summaries.sort_by(|left, right| left.source_id.cmp(&right.source_id));
        Ok(summaries)
    }

    fn model_receipt_path(&self, generation: u64) -> PathBuf {
        self.path
            .join("receipts")
            .join(format!("model-build-{generation:010}.json"))
    }

    fn view_path(&self, view_id: &str) -> PathBuf {
        self.path
            .join("store")
            .join("views")
            .join(format!("{}.json", safe_file_stem(view_id)))
    }

    fn view_receipt_path(&self, view_id: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "view-build-{}-{generation:010}.json",
            safe_file_stem(view_id)
        ))
    }

    fn candidate_cut_path(&self, cut_id: &str) -> PathBuf {
        self.path
            .join("cuts")
            .join(format!("{}.candidate.json", safe_file_stem(cut_id)))
    }

    fn closed_cut_path(&self, cut_id: &str) -> PathBuf {
        self.path
            .join("cuts")
            .join(format!("{}.closed.json", safe_file_stem(cut_id)))
    }

    fn cut_receipt_path(&self, cut_id: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "cut-build-{}-{generation:010}.json",
            safe_file_stem(cut_id)
        ))
    }

    fn close_receipt_path(&self, cut_id: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "close-{}-{generation:010}.json",
            safe_file_stem(cut_id)
        ))
    }

    fn pack_path(&self, cut_id: &str) -> PathBuf {
        self.path
            .join("packs")
            .join(format!("{}.pack.json", safe_file_stem(cut_id)))
    }

    fn prompt_path(&self, cut_id: &str) -> PathBuf {
        self.path
            .join("packs")
            .join(format!("{}.prompt-frame.json", safe_file_stem(cut_id)))
    }

    fn press_frame_path(&self, cut_id: &str) -> PathBuf {
        self.path
            .join("packs")
            .join(format!("{}.press-frame.json", safe_file_stem(cut_id)))
    }

    fn cache_manifest_path(&self, kind: &str, cut_id: &str) -> PathBuf {
        self.path.join("cache").join(format!(
            "{}.{}.cache-manifest.json",
            safe_file_stem(cut_id),
            safe_file_stem(kind)
        ))
    }

    fn handoff_receipt_path(&self, command: &str, cut_id: &str, generation: u64) -> PathBuf {
        self.path.join("receipts").join(format!(
            "{}-{}-{generation:010}.json",
            safe_file_stem(command),
            safe_file_stem(cut_id)
        ))
    }

    fn read_view_record(&self, view_id: &str) -> Result<WorkspaceViewRecord, WorkspaceError> {
        let path = self.view_path(view_id);
        if !path.exists() {
            return Err(WorkspaceError::MissingView {
                view_id: view_id.to_string(),
            });
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| WorkspaceError::io("read view", path.clone(), error))?;
        Ok(WorkspaceViewRecord {
            view_id: json_string_field(&path, &contents, "view_id")?,
            filter: json_string_field(&path, &contents, "filter")?,
            source_ids: json_string_array_field(&path, &contents, "source_ids")?,
        })
    }

    fn read_candidate_cut_record(
        &self,
        cut_id: &str,
    ) -> Result<WorkspaceCandidateCutRecord, WorkspaceError> {
        let path = self.candidate_cut_path(cut_id);
        if !path.exists() {
            return Err(WorkspaceError::MissingCandidateCut {
                cut_id: cut_id.to_string(),
            });
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| WorkspaceError::io("read candidate cut", path.clone(), error))?;
        Ok(WorkspaceCandidateCutRecord {
            cut_id: json_string_field(&path, &contents, "cut_id")?,
            view_id: json_string_field(&path, &contents, "view_id")?,
            source_ids: json_string_array_field(&path, &contents, "source_ids")?,
            token_budget: json_u64_field(&path, &contents, "token_budget")?,
            estimated_tokens: json_u64_field(&path, &contents, "estimated_tokens")?,
        })
    }

    fn read_ingest_records_for_sources(
        &self,
        source_ids: &[String],
    ) -> Result<Vec<IngestRecord>, WorkspaceError> {
        let mut records = Vec::new();
        for source_id in source_ids {
            let path = self.ingest_record_path(source_id);
            let contents = fs::read_to_string(&path)
                .map_err(|error| WorkspaceError::io("read ingest record", path.clone(), error))?;
            records.push(ingest_record_from_json(&path, &contents)?);
        }
        Ok(records)
    }

    fn ensure_parent(&self, path: &Path, action: &'static str) -> Result<(), WorkspaceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| WorkspaceError::io(action, parent, error))?;
        }
        Ok(())
    }

    fn read_workspace_closed_cut(&self, cut_id: &str) -> Result<ClosedCut, WorkspaceError> {
        let path = self.closed_cut_path(cut_id);
        if !path.exists() {
            return Err(WorkspaceError::MissingClosedCut {
                cut_id: cut_id.to_string(),
            });
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| WorkspaceError::io("read closed cut", path.clone(), error))?;
        closed_cut_from_json(&path, &contents)
    }

    fn write_model_receipt(
        &self,
        generation: u64,
        ingest_record_count: usize,
        grain_count: usize,
        bond_count: usize,
        receipt_count: usize,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"model-build\",",
                "\"generation\":{},",
                "\"status\":\"committed\",",
                "\"ingest_record_count\":{},",
                "\"grain_count\":{},",
                "\"bond_count\":{},",
                "\"receipt_count\":{}",
                "}}\n"
            ),
            generation, ingest_record_count, grain_count, bond_count, receipt_count
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn write_view_receipt(
        &self,
        generation: u64,
        view_id: &str,
        filter: &str,
        source_count: usize,
        grain_count: usize,
        bond_count: usize,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"view-build\",",
                "\"generation\":{},",
                "\"status\":\"committed\",",
                "\"view_id\":\"{}\",",
                "\"filter\":\"{}\",",
                "\"source_count\":{},",
                "\"grain_count\":{},",
                "\"bond_count\":{}",
                "}}\n"
            ),
            generation,
            json_escape(view_id),
            json_escape(filter),
            source_count,
            grain_count,
            bond_count
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn write_cut_receipt(
        &self,
        generation: u64,
        cut_id: &str,
        view_id: &str,
        source_count: usize,
        grain_count: usize,
        bond_count: usize,
        token_budget: u64,
        estimated_tokens: u64,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"candidate-cut-build\",",
                "\"generation\":{},",
                "\"status\":\"committed\",",
                "\"cut_id\":\"{}\",",
                "\"view_id\":\"{}\",",
                "\"candidate_only\":true,",
                "\"source_count\":{},",
                "\"grain_count\":{},",
                "\"bond_count\":{},",
                "\"token_budget\":{},",
                "\"estimated_tokens\":{}",
                "}}\n"
            ),
            generation,
            json_escape(cut_id),
            json_escape(view_id),
            source_count,
            grain_count,
            bond_count,
            token_budget,
            estimated_tokens
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn write_close_receipt(
        &self,
        generation: u64,
        cut_id: &str,
        cut_hash: &str,
        receipt_hash: &str,
        budget_status: &str,
        grain_count: usize,
        bond_count: usize,
        receipt_count: usize,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"close\",",
                "\"generation\":{},",
                "\"status\":\"committed\",",
                "\"cut_id\":\"{}\",",
                "\"closed\":true,",
                "\"cut_hash\":\"{}\",",
                "\"receipt_hash\":\"{}\",",
                "\"budget_status\":\"{}\",",
                "\"grain_count\":{},",
                "\"bond_count\":{},",
                "\"receipt_count\":{}",
                "}}\n"
            ),
            generation,
            json_escape(cut_id),
            json_escape(cut_hash),
            json_escape(receipt_hash),
            json_escape(budget_status),
            grain_count,
            bond_count,
            receipt_count
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn write_handoff_receipt(
        &self,
        generation: u64,
        kind: &str,
        cut_id: &str,
        output_id: &str,
        cut_hash: &str,
        receipt_hash: &str,
        cache_prefix: &str,
        cache_manifest_path: &Path,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = format!(
            concat!(
                "{{",
                "\"schema\":\"lattice.receipt.v1\",",
                "\"kind\":\"{}\",",
                "\"generation\":{},",
                "\"status\":\"committed\",",
                "\"cut_id\":\"{}\",",
                "\"output_id\":\"{}\",",
                "\"cut_hash\":\"{}\",",
                "\"receipt_hash\":\"{}\",",
                "\"cache_prefix\":\"{}\",",
                "\"cache_manifest_path\":\"{}\",",
                "\"materialized\":true,",
                "\"renders_artifact\":false",
                "}}\n"
            ),
            json_escape(kind),
            generation,
            json_escape(cut_id),
            json_escape(output_id),
            json_escape(cut_hash),
            json_escape(receipt_hash),
            json_escape(cache_prefix),
            json_escape(&cache_manifest_path.display().to_string())
        );
        fs::write(path, body).map_err(|error| WorkspaceError::io("write receipt", path, error))
    }

    fn write_cache_manifest(
        &self,
        generation: u64,
        kind: &str,
        cut_id: &str,
        output_id: &str,
        cut_hash: &str,
        receipt_hash: &str,
        cache_prefix: &str,
        output_path: &Path,
        receipt_path: &Path,
        manifest_path: &Path,
    ) -> Result<(), WorkspaceError> {
        let body = cache_manifest_json(
            generation,
            kind,
            cut_id,
            output_id,
            cut_hash,
            receipt_hash,
            cache_prefix,
            output_path,
            receipt_path,
        );
        fs::write(manifest_path, body).map_err(|error| {
            WorkspaceError::io("write cache manifest", manifest_path.to_path_buf(), error)
        })
    }
}

struct WorkspaceWriterLock {
    path: PathBuf,
}

impl WorkspaceWriterLock {
    fn acquire(workspace_path: &Path) -> Result<Self, WorkspaceError> {
        let path = workspace_path.join(WRITER_LOCK_FILE);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| WorkspaceError::io("acquire writer lock", path.clone(), error))?;
        file.write_all(b"locked\n")
            .map_err(|error| WorkspaceError::io("write writer lock", path.clone(), error))?;
        Ok(Self { path })
    }
}

impl Drop for WorkspaceWriterLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn source_pointer_row(pointer: &SourcePointer) -> String {
    [
        pointer.source_id.as_str(),
        pointer.owner_repo.as_str(),
        pointer.work_id.as_str(),
        pointer.fletch_registry_path.as_str(),
        pointer.fletch_id.as_str(),
        pointer.proof_ledger_path.as_str(),
        pointer.proof_record_path.as_str(),
        pointer.rights_policy.as_str(),
        pointer.rights_boundary.as_str(),
        pointer.refresh_status.as_str(),
        pointer.refresh_check.as_str(),
        pointer.custody_owner.as_str(),
        pointer.custody_distributor.as_str(),
    ]
    .into_iter()
    .map(encode_field)
    .collect::<Vec<_>>()
    .join("\t")
        + "\n"
}

/// Parse a `source-pointers.tsv` row.
///
/// Accepts two shapes:
///
/// - **13 columns** (current): every column read directly from the row.
/// - **9 columns** (legacy v1, pre-L06-follow-on): the four custody columns
///   added by the L06 follow-on (`refresh_status`, `refresh_check`,
///   `custody_owner`, `custody_distributor`) are filled with the documented
///   migration sentinel
///   [`lattice_registry::SOURCE_POINTER_LEGACY_CUSTODY_FIELD`] so existing
///   workspaces keep loading without rewriting.
///
/// Any other column count is reported as `InvalidRegistryRow`.
fn source_pointer_from_row(
    path: &Path,
    line_number: usize,
    row: &str,
) -> Result<SourcePointer, WorkspaceError> {
    let fields = row.split('\t').map(decode_field).collect::<Vec<_>>();
    match fields.len() {
        13 => Ok(SourcePointer::with_custody(
            fields[0].clone(),
            fields[1].clone(),
            fields[2].clone(),
            fields[3].clone(),
            fields[4].clone(),
            fields[5].clone(),
            fields[6].clone(),
            fields[7].clone(),
            fields[8].clone(),
            fields[9].clone(),
            fields[10].clone(),
            fields[11].clone(),
            fields[12].clone(),
        )),
        9 => Ok(SourcePointer::with_custody(
            fields[0].clone(),
            fields[1].clone(),
            fields[2].clone(),
            fields[3].clone(),
            fields[4].clone(),
            fields[5].clone(),
            fields[6].clone(),
            fields[7].clone(),
            fields[8].clone(),
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD.to_string(),
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD.to_string(),
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD.to_string(),
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD.to_string(),
        )),
        other => Err(WorkspaceError::InvalidRegistryRow {
            path: path.to_path_buf(),
            line_number,
            message: format!("expected 9 or 13 fields, found {other}"),
        }),
    }
}

fn encode_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn decode_field(value: &str) -> String {
    let mut decoded = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('t') => decoded.push('\t'),
                Some('r') => decoded.push('\r'),
                Some('n') => decoded.push('\n'),
                Some('\\') => decoded.push('\\'),
                Some(other) => {
                    decoded.push('\\');
                    decoded.push(other);
                }
                None => decoded.push('\\'),
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

fn safe_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn pebble_escape(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '|' | '\n' | '\r' | '\t' => '_',
            _ => character,
        })
        .collect()
}

fn ingest_record_json(record: &IngestRecord) -> String {
    let grains = record
        .grains
        .iter()
        .map(grain_json)
        .collect::<Vec<_>>()
        .join(",");
    let bonds = record
        .bonds
        .iter()
        .map(bond_json)
        .collect::<Vec<_>>()
        .join(",");
    let receipts = record
        .receipts
        .iter()
        .map(|receipt| {
            format!(
                concat!(
                    "{{",
                    "\"rule\":\"{}\",",
                    "\"outcome\":\"{}\",",
                    "\"source_id\":\"{}\",",
                    "\"note\":\"{}\"",
                    "}}"
                ),
                json_escape(&receipt.rule),
                receipt.outcome.as_str(),
                json_escape(&receipt.source_id),
                json_escape(&receipt.note)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.ingest-record.v1\",",
            "\"source_id\":\"{}\",",
            "\"owner_repo\":\"{}\",",
            "\"work_id\":\"{}\",",
            "\"rights_policy\":\"{}\",",
            "\"rights_boundary\":\"{}\",",
            "\"outcome\":\"{}\",",
            "\"grain_count\":{},",
            "\"bond_count\":{},",
            "\"receipt_count\":{},",
            "\"grains\":[{}],",
            "\"bonds\":[{}],",
            "\"receipts\":[{}]",
            "}}\n"
        ),
        json_escape(&record.source_id),
        json_escape(&record.owner_repo),
        json_escape(&record.work_id),
        json_escape(&record.rights_policy),
        json_escape(&record.rights_boundary),
        record.outcome.as_str(),
        record.grain_count(),
        record.bond_count(),
        record.receipts.len(),
        grains,
        bonds,
        receipts
    )
}

fn grain_json(grain: &Grain) -> String {
    format!(
        concat!(
            "{{",
            "\"id\":\"{}\",",
            "\"label\":\"{}\",",
            "\"kind\":\"{}\",",
            "\"source_id\":{},",
            "\"rights_policy\":{}",
            "}}"
        ),
        json_escape(grain.id.as_str()),
        json_escape(&grain.label),
        grain.kind.as_str(),
        optional_json_string(grain.source_id.as_deref()),
        optional_json_string(grain.rights_policy.as_deref())
    )
}

fn bond_json(bond: &Bond) -> String {
    format!(
        "{{\"from\":\"{}\",\"to\":\"{}\",\"kind\":\"{}\"}}",
        json_escape(bond.from.as_str()),
        json_escape(bond.to.as_str()),
        bond.kind.as_str()
    )
}

fn optional_json_string(value: Option<&str>) -> String {
    value
        .map(|value| format!("\"{}\"", json_escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ViewPredicate {
    field: String,
    op: String,
    literal: String,
}

impl ViewPredicate {
    fn matches(&self, summary: &IngestRecordSummary) -> bool {
        let value = match self.field.as_str() {
            "source" | "source_id" => summary.source_id.as_str(),
            "repo" => summary.owner_repo.as_str(),
            "rights" => summary.rights_policy.as_str(),
            _ => return false,
        };

        match self.op.as_str() {
            "eq" => value == self.literal,
            "neq" => value != self.literal,
            "contains" => value.contains(&self.literal),
            _ => false,
        }
    }
}

fn parse_filter(filter: &str) -> Result<Vec<ViewPredicate>, WorkspaceError> {
    let filter = filter.trim();
    if filter.is_empty() {
        return Ok(Vec::new());
    }

    filter
        .split(" and ")
        .map(|raw| parse_predicate(filter, raw.trim()))
        .collect()
}

fn parse_predicate(filter: &str, raw: &str) -> Result<ViewPredicate, WorkspaceError> {
    let parts = raw.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(WorkspaceError::InvalidViewFilter {
            filter: filter.to_string(),
            message: format!("predicate {raw:?} must be <field> <op> 'literal'"),
        });
    }
    let field = parts[0];
    let op = parts[1];
    let literal = parts[2];
    if !matches!(field, "source" | "source_id" | "repo" | "rights") {
        return Err(WorkspaceError::InvalidViewFilter {
            filter: filter.to_string(),
            message: format!("unknown field {field:?}"),
        });
    }
    if !matches!(op, "eq" | "neq" | "contains") {
        return Err(WorkspaceError::InvalidViewFilter {
            filter: filter.to_string(),
            message: format!("unknown operator {op:?}"),
        });
    }
    if !(literal.starts_with('\'') && literal.ends_with('\'') && literal.len() >= 2) {
        return Err(WorkspaceError::InvalidViewFilter {
            filter: filter.to_string(),
            message: "literal must be single-quoted".to_string(),
        });
    }
    Ok(ViewPredicate {
        field: field.to_string(),
        op: op.to_string(),
        literal: literal[1..literal.len() - 1].to_string(),
    })
}

fn ingest_summary_from_json(
    path: &Path,
    contents: &str,
) -> Result<IngestRecordSummary, WorkspaceError> {
    Ok(IngestRecordSummary {
        source_id: json_string_field(path, contents, "source_id")?,
        owner_repo: json_string_field(path, contents, "owner_repo")?,
        rights_policy: json_string_field(path, contents, "rights_policy")?,
        grain_count: json_usize_field(path, contents, "grain_count")?,
        bond_count: json_usize_field(path, contents, "bond_count")?,
        receipt_count: json_usize_field(path, contents, "receipt_count")?,
        path: path.to_path_buf(),
    })
}

fn ingest_record_from_json(path: &Path, contents: &str) -> Result<IngestRecord, WorkspaceError> {
    let outcome = match json_string_field(path, contents, "outcome")?.as_str() {
        "normalized" => IngestOutcome::Normalized,
        "skipped" => IngestOutcome::Skipped,
        "rejected" => IngestOutcome::Rejected,
        other => {
            return Err(invalid_json(
                path,
                format!("unknown ingest outcome {other:?}"),
            ));
        }
    };
    Ok(IngestRecord {
        source_id: json_string_field(path, contents, "source_id")?,
        owner_repo: json_string_field(path, contents, "owner_repo")?,
        work_id: json_string_field(path, contents, "work_id")?,
        rights_policy: json_string_field(path, contents, "rights_policy")?,
        rights_boundary: json_string_field(path, contents, "rights_boundary")?,
        outcome,
        grains: grains_from_json(path, contents)?,
        bonds: bonds_from_json(path, contents)?,
        receipts: ingest_receipts_from_json(path, contents)?,
    })
}

fn grains_from_json(path: &Path, contents: &str) -> Result<Vec<Grain>, WorkspaceError> {
    json_object_array_field(path, contents, "grains")?
        .into_iter()
        .map(|object| {
            let kind = match json_string_field(path, &object, "kind")?.as_str() {
                "source_pointer" => GrainKind::SourcePointer,
                "context" => GrainKind::Context,
                "evidence" => GrainKind::Evidence,
                "policy" => GrainKind::Policy,
                "receipt" => GrainKind::Receipt,
                other => return Err(invalid_json(path, format!("unknown grain kind {other:?}"))),
            };
            Ok(Grain {
                id: GrainId::new(json_string_field(path, &object, "id")?),
                label: json_string_field(path, &object, "label")?,
                kind,
                source_id: json_optional_string_field(path, &object, "source_id")?,
                rights_policy: json_optional_string_field(path, &object, "rights_policy")?,
            })
        })
        .collect()
}

fn bonds_from_json(path: &Path, contents: &str) -> Result<Vec<Bond>, WorkspaceError> {
    json_object_array_field(path, contents, "bonds")?
        .into_iter()
        .map(|object| {
            let kind = match json_string_field(path, &object, "kind")?.as_str() {
                "contains" => BondKind::Contains,
                "derives_from" => BondKind::DerivesFrom,
                "cites" => BondKind::Cites,
                "contradicts" => BondKind::Contradicts,
                "same_entity" => BondKind::SameEntity,
                "requires" => BondKind::Requires,
                other => return Err(invalid_json(path, format!("unknown bond kind {other:?}"))),
            };
            Ok(Bond::new(
                GrainId::new(json_string_field(path, &object, "from")?),
                GrainId::new(json_string_field(path, &object, "to")?),
                kind,
            ))
        })
        .collect()
}

fn ingest_receipts_from_json(
    path: &Path,
    contents: &str,
) -> Result<Vec<IngestReceipt>, WorkspaceError> {
    json_object_array_field(path, contents, "receipts")?
        .into_iter()
        .map(|object| {
            let outcome = match json_string_field(path, &object, "outcome")?.as_str() {
                "normalized" => IngestOutcome::Normalized,
                "skipped" => IngestOutcome::Skipped,
                "rejected" => IngestOutcome::Rejected,
                other => {
                    return Err(invalid_json(
                        path,
                        format!("unknown ingest receipt outcome {other:?}"),
                    ));
                }
            };
            Ok(IngestReceipt::new(
                json_string_field(path, &object, "rule")?,
                outcome,
                json_string_field(path, &object, "source_id")?,
                json_string_field(path, &object, "note")?,
            ))
        })
        .collect()
}

fn closure_receipts_from_json(
    path: &Path,
    contents: &str,
) -> Result<Vec<ClosureReceipt>, WorkspaceError> {
    json_object_array_field(path, contents, "closure_receipts")?
        .into_iter()
        .map(|object| {
            Ok(ClosureReceipt::new(
                json_string_field(path, &object, "rule")?,
                json_string_field(path, &object, "note")?,
            ))
        })
        .collect()
}

fn closed_cut_from_json(path: &Path, contents: &str) -> Result<ClosedCut, WorkspaceError> {
    let grains = grains_from_json(path, contents)?;
    let bonds = bonds_from_json(path, contents)?;
    let mut cut = ClosedCut::new(CutId::new(json_string_field(path, contents, "cut_id")?));
    for grain in grains {
        cut.grains.insert(grain.id.clone());
        cut.grain_records.push(grain);
    }
    for bond in bonds {
        cut.bonds.insert(bond);
    }
    for receipt in closure_receipts_from_json(path, contents)? {
        cut.closure_receipts.push(receipt);
    }
    cut.closure_policy = Some(json_string_field(path, contents, "closure_policy")?);
    cut.rights_policy = Some(json_string_field(path, contents, "rights_policy")?);
    Ok(cut)
}

fn json_string_field(path: &Path, contents: &str, field: &str) -> Result<String, WorkspaceError> {
    let marker = format!("\"{field}\":\"");
    let start = contents
        .find(&marker)
        .ok_or_else(|| invalid_json(path, format!("missing string field {field:?}")))?
        + marker.len();
    let rest = &contents[start..];
    let end = rest
        .find('"')
        .ok_or_else(|| invalid_json(path, format!("unterminated string field {field:?}")))?;
    Ok(rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn json_optional_string_field(
    path: &Path,
    contents: &str,
    field: &str,
) -> Result<Option<String>, WorkspaceError> {
    let string_marker = format!("\"{field}\":\"");
    if contents.contains(&string_marker) {
        return json_string_field(path, contents, field).map(Some);
    }
    let null_marker = format!("\"{field}\":null");
    if contents.contains(&null_marker) {
        return Ok(None);
    }
    Err(invalid_json(
        path,
        format!("missing nullable string field {field:?}"),
    ))
}

fn json_usize_field(path: &Path, contents: &str, field: &str) -> Result<usize, WorkspaceError> {
    let marker = format!("\"{field}\":");
    let start = contents
        .find(&marker)
        .ok_or_else(|| invalid_json(path, format!("missing numeric field {field:?}")))?
        + marker.len();
    let rest = &contents[start..];
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end]
        .parse()
        .map_err(|_| invalid_json(path, format!("invalid numeric field {field:?}")))
}

fn json_u64_field(path: &Path, contents: &str, field: &str) -> Result<u64, WorkspaceError> {
    let marker = format!("\"{field}\":");
    let start = contents
        .find(&marker)
        .ok_or_else(|| invalid_json(path, format!("missing numeric field {field:?}")))?
        + marker.len();
    let rest = &contents[start..];
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end]
        .parse()
        .map_err(|_| invalid_json(path, format!("invalid numeric field {field:?}")))
}

fn json_string_array_field(
    path: &Path,
    contents: &str,
    field: &str,
) -> Result<Vec<String>, WorkspaceError> {
    let marker = format!("\"{field}\":[");
    let start = contents
        .find(&marker)
        .ok_or_else(|| invalid_json(path, format!("missing array field {field:?}")))?
        + marker.len();
    let rest = &contents[start..];
    let end = rest
        .find(']')
        .ok_or_else(|| invalid_json(path, format!("unterminated array field {field:?}")))?;
    let body = rest[..end].trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    Ok(body
        .split(',')
        .map(|value| {
            value
                .trim()
                .trim_matches('"')
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
        })
        .collect())
}

fn json_object_array_field(
    path: &Path,
    contents: &str,
    field: &str,
) -> Result<Vec<String>, WorkspaceError> {
    let marker = format!("\"{field}\":[");
    let start = contents
        .find(&marker)
        .ok_or_else(|| invalid_json(path, format!("missing object array field {field:?}")))?
        + marker.len();
    let rest = &contents[start..];
    let end = matching_array_end(rest)
        .ok_or_else(|| invalid_json(path, format!("unterminated object array field {field:?}")))?;
    split_json_objects(path, &rest[..end])
}

fn matching_array_end(value: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '[' if !in_string => depth += 1,
            ']' if !in_string && depth == 0 => return Some(index),
            ']' if !in_string => depth -= 1,
            _ => {}
        }
    }
    None
}

fn split_json_objects(path: &Path, body: &str) -> Result<Vec<String>, WorkspaceError> {
    let body = body.trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let mut objects = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' if !in_string => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    invalid_json(path, "unexpected object terminator in array".to_string())
                })?;
                if depth == 0 {
                    let start = start.ok_or_else(|| {
                        invalid_json(path, "object end without object start".to_string())
                    })?;
                    objects.push(body[start..=index].to_string());
                }
            }
            _ => {}
        }
    }
    if depth != 0 || in_string {
        return Err(invalid_json(
            path,
            "unterminated object in array".to_string(),
        ));
    }
    Ok(objects)
}

fn invalid_json(path: &Path, message: String) -> WorkspaceError {
    WorkspaceError::InvalidRegistryRow {
        path: path.to_path_buf(),
        line_number: 0,
        message,
    }
}

fn model_json(summaries: &[IngestRecordSummary]) -> String {
    let sources = summaries
        .iter()
        .map(|summary| {
            format!(
                "{{\"source_id\":\"{}\",\"owner_repo\":\"{}\",\"rights_policy\":\"{}\",\"grain_count\":{},\"bond_count\":{},\"receipt_count\":{},\"record_path\":\"{}\"}}",
                json_escape(&summary.source_id),
                json_escape(&summary.owner_repo),
                json_escape(&summary.rights_policy),
                summary.grain_count,
                summary.bond_count,
                summary.receipt_count,
                json_escape(&summary.path.display().to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let grain_count = summaries
        .iter()
        .map(|summary| summary.grain_count)
        .sum::<usize>();
    let bond_count = summaries
        .iter()
        .map(|summary| summary.bond_count)
        .sum::<usize>();
    let receipt_count = summaries
        .iter()
        .map(|summary| summary.receipt_count)
        .sum::<usize>();
    format!(
        "{{\"schema\":\"lattice.model.v1\",\"source_count\":{},\"grain_count\":{},\"bond_count\":{},\"receipt_count\":{},\"sources\":[{}]}}\n",
        summaries.len(),
        grain_count,
        bond_count,
        receipt_count,
        sources
    )
}

fn view_json(
    view_id: &str,
    filter: &str,
    predicate_count: usize,
    source_ids: &[String],
    grain_count: usize,
    bond_count: usize,
) -> String {
    let source_ids = source_ids
        .iter()
        .map(|source_id| format!("\"{}\"", json_escape(source_id)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"lattice.view.v1\",\"view_id\":\"{}\",\"filter\":\"{}\",\"predicate_count\":{},\"candidate_only\":true,\"source_count\":{},\"grain_count\":{},\"bond_count\":{},\"source_ids\":[{}]}}\n",
        json_escape(view_id),
        json_escape(filter),
        predicate_count,
        if source_ids.is_empty() { 0 } else { source_ids.split(',').count() },
        grain_count,
        bond_count,
        source_ids
    )
}

fn candidate_cut_json(
    cut_id: &str,
    view_id: &str,
    filter: &str,
    token_budget: u64,
    estimated_tokens: u64,
    source_ids: &[String],
    grain_count: usize,
    bond_count: usize,
) -> String {
    let source_ids = source_ids
        .iter()
        .map(|source_id| format!("\"{}\"", json_escape(source_id)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"lattice.candidate-cut.v1\",\"cut_id\":\"{}\",\"view_id\":\"{}\",\"filter\":\"{}\",\"candidate_only\":true,\"closed\":false,\"token_budget\":{},\"estimated_tokens\":{},\"source_count\":{},\"grain_count\":{},\"bond_count\":{},\"source_ids\":[{}]}}\n",
        json_escape(cut_id),
        json_escape(view_id),
        json_escape(filter),
        token_budget,
        estimated_tokens,
        source_ids.split(',').filter(|value| !value.is_empty()).count(),
        grain_count,
        bond_count,
        source_ids
    )
}

fn closed_cut_json(
    cut: &ClosedCut,
    candidate: &WorkspaceCandidateCutRecord,
    cut_hash: &str,
    receipt_hash: &str,
    budget_status: &str,
    frontier_count: usize,
) -> String {
    let grains = cut
        .grain_records
        .iter()
        .map(grain_json)
        .collect::<Vec<_>>()
        .join(",");
    let bonds = cut
        .bonds
        .iter()
        .map(bond_json)
        .collect::<Vec<_>>()
        .join(",");
    let receipts = cut
        .closure_receipts
        .iter()
        .map(|receipt| {
            format!(
                "{{\"rule\":\"{}\",\"note\":\"{}\",\"receipt_hash\":\"{}\"}}",
                json_escape(&receipt.rule),
                json_escape(&receipt.note),
                json_escape(&receipt.stable_hash())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let source_ids = candidate
        .source_ids
        .iter()
        .map(|source_id| format!("\"{}\"", json_escape(source_id)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.closed-cut.v1\",",
            "\"cut_id\":\"{}\",",
            "\"view_id\":\"{}\",",
            "\"candidate_only\":false,",
            "\"closed\":true,",
            "\"cut_hash\":\"{}\",",
            "\"receipt_hash\":\"{}\",",
            "\"closure_policy\":\"{}\",",
            "\"rights_policy\":\"{}\",",
            "\"budget_status\":\"{}\",",
            "\"token_budget\":{},",
            "\"estimated_tokens\":{},",
            "\"source_count\":{},",
            "\"grain_count\":{},",
            "\"bond_count\":{},",
            "\"receipt_count\":{},",
            "\"frontier_count\":{},",
            "\"source_ids\":[{}],",
            "\"grains\":[{}],",
            "\"bonds\":[{}],",
            "\"closure_receipts\":[{}]",
            "}}\n"
        ),
        json_escape(cut.id.as_str()),
        json_escape(&candidate.view_id),
        json_escape(cut_hash),
        json_escape(receipt_hash),
        json_escape(cut.closure_policy.as_deref().unwrap_or("")),
        json_escape(cut.rights_policy.as_deref().unwrap_or("")),
        json_escape(budget_status),
        candidate.token_budget,
        candidate.estimated_tokens,
        candidate.source_ids.len(),
        cut.grains.len(),
        cut.bonds.len(),
        cut.closure_receipts.len(),
        frontier_count,
        source_ids,
        grains,
        bonds,
        receipts
    )
}

fn pack_metadata_json(pack: &ContextPack) -> String {
    let caveats = pack
        .caveats
        .iter()
        .map(|caveat| format!("\"{}\"", json_escape(caveat)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.pack.v1\",",
            "\"pack_id\":\"{}\",",
            "\"cut_id\":\"{}\",",
            "\"cut_hash\":\"{}\",",
            "\"receipt_hash\":\"{}\",",
            "\"profile_id\":\"{}\",",
            "\"cache_prefix\":\"{}\",",
            "\"grain_count\":{},",
            "\"bond_count\":{},",
            "\"receipt_count\":{},",
            "\"closure_policy\":\"{}\",",
            "\"rights_policy\":\"{}\",",
            "\"materialized\":true,",
            "\"renders_artifact\":false,",
            "\"caveats\":[{}]",
            "}}\n"
        ),
        json_escape(&pack.id),
        json_escape(pack.cut_id.as_str()),
        json_escape(&pack.cut_hash),
        json_escape(&pack.receipt_hash),
        json_escape(&pack.profile_id),
        json_escape(&pack.cache_prefix),
        pack.grain_count,
        pack.bond_count,
        pack.receipt_count,
        json_escape(pack.closure_policy.as_deref().unwrap_or("unknown")),
        json_escape(pack.rights_policy.as_deref().unwrap_or("unknown")),
        caveats
    )
}

fn prompt_frame_json(prompt: &PromptFrame) -> String {
    let grain_ids = prompt
        .grain_ids
        .iter()
        .map(|grain| format!("\"{}\"", json_escape(grain)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.prompt-frame.v1\",",
            "\"frame_version\":\"{}\",",
            "\"pack_id\":\"{}\",",
            "\"cut_id\":\"{}\",",
            "\"cut_hash\":\"{}\",",
            "\"receipt_hash\":\"{}\",",
            "\"profile_id\":\"{}\",",
            "\"cache_prefix\":\"{}\",",
            "\"contract\":\"{}\",",
            "\"grain_count\":{},",
            "\"receipt_count\":{},",
            "\"materialized\":true,",
            "\"renders_artifact\":false,",
            "\"grain_ids\":[{}]",
            "}}\n"
        ),
        json_escape(&prompt.frame_version),
        json_escape(&prompt.pack.id),
        json_escape(prompt.pack.cut_id.as_str()),
        json_escape(&prompt.pack.cut_hash),
        json_escape(&prompt.pack.receipt_hash),
        json_escape(&prompt.pack.profile_id),
        json_escape(&prompt.pack.cache_prefix),
        json_escape(&prompt.contract),
        prompt.grain_ids.len(),
        prompt.receipt_count,
        grain_ids
    )
}

fn press_frame_metadata_json(frame: &PressPublicationFrame) -> String {
    let caveats = frame
        .caveats
        .iter()
        .map(|caveat| format!("\"{}\"", json_escape(caveat)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.press-frame.v1\",",
            "\"frame_version\":\"{}\",",
            "\"pack_id\":\"{}\",",
            "\"cut_id\":\"{}\",",
            "\"cut_hash\":\"{}\",",
            "\"receipt_hash\":\"{}\",",
            "\"cache_prefix\":\"{}\",",
            "\"target_family\":\"{}\",",
            "\"handoff_contract\":\"{}\",",
            "\"receipt_count\":{},",
            "\"materialized\":true,",
            "\"renders_artifact\":false,",
            "\"caveats\":[{}]",
            "}}\n"
        ),
        json_escape(&frame.frame_version),
        json_escape(&frame.pack.id),
        json_escape(frame.pack.cut_id.as_str()),
        json_escape(&frame.pack.cut_hash),
        json_escape(&frame.pack.receipt_hash),
        json_escape(&frame.pack.cache_prefix),
        json_escape(&frame.target_family),
        json_escape(&frame.handoff_contract),
        frame.receipt_count,
        caveats
    )
}

fn cache_manifest_json(
    generation: u64,
    kind: &str,
    cut_id: &str,
    output_id: &str,
    cut_hash: &str,
    receipt_hash: &str,
    cache_prefix: &str,
    output_path: &Path,
    receipt_path: &Path,
) -> String {
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.cache-manifest.v1\",",
            "\"kind\":\"{}\",",
            "\"generation\":{},",
            "\"cut_id\":\"{}\",",
            "\"output_id\":\"{}\",",
            "\"cut_hash\":\"{}\",",
            "\"receipt_hash\":\"{}\",",
            "\"cache_prefix\":\"{}\",",
            "\"output_path\":\"{}\",",
            "\"receipt_path\":\"{}\",",
            "\"materialized\":true,",
            "\"renders_artifact\":false",
            "}}\n"
        ),
        json_escape(kind),
        generation,
        json_escape(cut_id),
        json_escape(output_id),
        json_escape(cut_hash),
        json_escape(receipt_hash),
        json_escape(cache_prefix),
        json_escape(&output_path.display().to_string()),
        json_escape(&receipt_path.display().to_string())
    )
}

fn source_corpus_index_json(
    generation: u64,
    summaries: &[IngestRecordSummary],
    index_entry_count: usize,
) -> String {
    let entries = summaries
        .iter()
        .map(|summary| {
            format!(
                concat!(
                    "{{",
                    "\"source_id\":\"{}\",",
                    "\"owner_repo\":\"{}\",",
                    "\"rights_policy\":\"{}\",",
                    "\"grain_count\":{},",
                    "\"bond_count\":{},",
                    "\"receipt_count\":{}",
                    "}}"
                ),
                json_escape(&summary.source_id),
                json_escape(&summary.owner_repo),
                json_escape(&summary.rights_policy),
                summary.grain_count,
                summary.bond_count,
                summary.receipt_count
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.source-corpus-index.v1\",",
            "\"generation\":{},",
            "\"ingest_record_count\":{},",
            "\"source_count\":{},",
            "\"grain_count\":{},",
            "\"bond_count\":{},",
            "\"receipt_count\":{},",
            "\"index_entry_count\":{},",
            "\"entries\":[{}]",
            "}}\n"
        ),
        generation,
        summaries.len(),
        summaries.len(),
        summaries
            .iter()
            .map(|summary| summary.grain_count)
            .sum::<usize>(),
        summaries
            .iter()
            .map(|summary| summary.bond_count)
            .sum::<usize>(),
        summaries
            .iter()
            .map(|summary| summary.receipt_count)
            .sum::<usize>(),
        index_entry_count,
        entries
    )
}

fn source_corpus_index_artifact(
    generation: u64,
    summaries: &[IngestRecordSummary],
    index_entry_count: usize,
    artifact_profile: ArtifactProfile,
) -> String {
    match artifact_profile {
        ArtifactProfile::Audit => {
            source_corpus_index_json(generation, summaries, index_entry_count)
        }
        ArtifactProfile::Compact => {
            source_corpus_index_compact_json(generation, summaries, index_entry_count)
        }
        ArtifactProfile::Pebble => {
            source_corpus_index_pebble_lines(generation, summaries, index_entry_count)
        }
    }
}

fn source_corpus_index_path(workspace: &Path, artifact_profile: ArtifactProfile) -> PathBuf {
    let file_name = match artifact_profile {
        ArtifactProfile::Audit => "source-corpus-index.json",
        ArtifactProfile::Compact => "source-corpus-index.compact.json",
        ArtifactProfile::Pebble => "source-corpus-index.pebble.pbl",
    };
    workspace.join("cache").join(file_name)
}

fn source_corpus_index_compact_json(
    generation: u64,
    summaries: &[IngestRecordSummary],
    index_entry_count: usize,
) -> String {
    let entries = summaries
        .iter()
        .map(|summary| {
            format!(
                "[\"{}\",{},{},{}]",
                json_escape(&summary.source_id),
                summary.grain_count,
                summary.bond_count,
                summary.receipt_count
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{",
            "\"s\":\"lattice.source-corpus-index.compact.v1\",",
            "\"g\":{},",
            "\"c\":[{},{},{},{},{}],",
            "\"d\":{{\"c\":[\"source_id\",\"grain_count\",\"bond_count\",\"receipt_count\"]}},",
            "\"e\":[{}]",
            "}}\n"
        ),
        generation,
        summaries.len(),
        summaries
            .iter()
            .map(|summary| summary.grain_count)
            .sum::<usize>(),
        summaries
            .iter()
            .map(|summary| summary.bond_count)
            .sum::<usize>(),
        summaries
            .iter()
            .map(|summary| summary.receipt_count)
            .sum::<usize>(),
        index_entry_count,
        entries
    )
}

fn source_corpus_index_pebble_lines(
    generation: u64,
    summaries: &[IngestRecordSummary],
    index_entry_count: usize,
) -> String {
    let grain_count = summaries
        .iter()
        .map(|summary| summary.grain_count)
        .sum::<usize>();
    let bond_count = summaries
        .iter()
        .map(|summary| summary.bond_count)
        .sum::<usize>();
    let receipt_count = summaries
        .iter()
        .map(|summary| summary.receipt_count)
        .sum::<usize>();
    let mut lines = vec![format!(
        "PBL1|lattice.pebble-source-corpus-index.v1|g={generation}|sources={}|grains={grain_count}|bonds={bond_count}|receipts={receipt_count}|entries={index_entry_count}",
        summaries.len()
    )];
    lines.extend(summaries.iter().map(|summary| {
        format!(
            "S|{}|g{}|b{}|r{}",
            pebble_escape(&summary.source_id),
            summary.grain_count,
            summary.bond_count,
            summary.receipt_count
        )
    }));
    lines.push(String::new());
    lines.join("\n")
}

fn source_corpus_index_manifest_json(
    generation: u64,
    index_path: &Path,
    receipt_path: &Path,
    artifact_profile: ArtifactProfile,
    ingest_record_count: usize,
    grain_count: usize,
    bond_count: usize,
    receipt_count: usize,
    index_entry_count: usize,
) -> String {
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.cache-manifest.v1\",",
            "\"kind\":\"source-corpus-index\",",
            "\"generation\":{},",
            "\"artifact_profile\":\"{}\",",
            "\"artifact_format\":\"{}\",",
            "\"index_path\":\"{}\",",
            "\"receipt_path\":\"{}\",",
            "\"ingest_record_count\":{},",
            "\"grain_count\":{},",
            "\"bond_count\":{},",
            "\"receipt_count\":{},",
            "\"index_entry_count\":{},",
            "\"materialized\":true,",
            "\"renders_artifact\":false",
            "}}\n"
        ),
        generation,
        artifact_profile.as_str(),
        artifact_profile.format(),
        json_escape(&index_path.display().to_string()),
        json_escape(&receipt_path.display().to_string()),
        ingest_record_count,
        grain_count,
        bond_count,
        receipt_count,
        index_entry_count
    )
}

fn source_corpus_index_receipt_json(
    generation: u64,
    ingest_record_count: usize,
    artifact_profile: ArtifactProfile,
    grain_count: usize,
    bond_count: usize,
    receipt_count: usize,
    index_entry_count: usize,
) -> String {
    format!(
        concat!(
            "{{",
            "\"schema\":\"lattice.receipt.v1\",",
            "\"kind\":\"build-indexes\",",
            "\"generation\":{},",
            "\"status\":\"committed\",",
            "\"artifact_profile\":\"{}\",",
            "\"artifact_format\":\"{}\",",
            "\"ingest_record_count\":{},",
            "\"grain_count\":{},",
            "\"bond_count\":{},",
            "\"receipt_count\":{},",
            "\"index_entry_count\":{}",
            "}}\n"
        ),
        generation,
        artifact_profile.as_str(),
        artifact_profile.format(),
        ingest_record_count,
        grain_count,
        bond_count,
        receipt_count,
        index_entry_count
    )
}

fn estimate_tokens(grain_count: usize, bond_count: usize) -> u64 {
    ((grain_count * 24) + (bond_count * 8)) as u64
}

fn rights_policy_for_records(records: &[IngestRecord]) -> String {
    let mut policies = records
        .iter()
        .map(|record| record.rights_policy.as_str())
        .collect::<Vec<_>>();
    policies.sort_unstable();
    policies.dedup();
    match policies.as_slice() {
        [] => "unknown".to_string(),
        [policy] => (*policy).to_string(),
        _ => "mixed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stores_closed_cut_by_id() {
        let id = CutId::new("bottom");
        let cut = ClosedCut::new(id.clone());
        let mut store = MemoryStore::new();

        assert!(store.put_cut(cut).is_none());
        assert!(store.get_cut(&id).is_some());
        assert_eq!(store.cut_count(), 1);
    }

    #[test]
    fn materializes_rebuildable_pack_from_closed_cut() {
        let id = CutId::new("tiny-closed");
        let cut = ClosedCut::new(id.clone())
            .with_receipt(lattice_model::ClosureReceipt::new(
                "closed-cut",
                "closure verified",
            ))
            .with_policy("tiny-policy", "derived_text_allowed");
        let mut store = MemoryStore::new();

        store.put_cut(cut);
        let pack = store
            .materialize_pack_from_cut(&id, "press-handoff")
            .expect("cut should materialize");

        assert_eq!(pack.id, "pack:press-handoff:tiny-closed");
        assert_eq!(pack.receipt_count, 1);
        assert_eq!(pack.rights_policy.as_deref(), Some("derived_text_allowed"));
        assert!(store.get_pack("pack:press-handoff:tiny-closed").is_some());
        assert_eq!(store.pack_count(), 1);
    }

    #[test]
    fn initializes_file_backed_workspace_layout() {
        let path = temp_workspace_path("init-layout");

        let status = Workspace::init(&path).expect("workspace should initialize");

        assert_eq!(status.generation, 1);
        assert!(status.is_complete());
        assert_eq!(status.directory_count(), 6);
        assert!(path.join("generation").is_file());
        assert!(path
            .join("receipts")
            .join("workspace-init-0000000001.json")
            .is_file());

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn workspace_status_reads_generation_and_directory_presence() {
        let path = temp_workspace_path("status");
        Workspace::init(&path).expect("workspace should initialize");

        let status = Workspace::status(&path).expect("workspace status should load");

        assert_eq!(status.generation, 1);
        assert!(status.is_complete());

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn workspace_init_uses_generation_counter() {
        let path = temp_workspace_path("generation");
        Workspace::init(&path).expect("workspace should initialize");

        let status = Workspace::init(&path).expect("second init should commit next generation");

        assert_eq!(status.generation, 2);
        assert!(path
            .join("receipts")
            .join("workspace-init-0000000002.json")
            .is_file());

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn registers_and_lists_source_pointers() {
        let path = temp_workspace_path("registry");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = valid_pointer("fontes:apache-calcite:query-planning");

        let add = Workspace::add_source_pointer(&path, pointer.clone())
            .expect("source pointer should register");
        let list = Workspace::list_source_pointers(&path).expect("source pointers should list");

        assert_eq!(add.generation, 2);
        assert_eq!(add.source_id, pointer.source_id);
        assert!(add.registry_path.is_file());
        assert!(add.receipt_path.is_file());
        assert_eq!(list.generation, 2);
        assert_eq!(list.pointers, vec![pointer]);

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn invalid_source_pointer_writes_failure_receipt() {
        let path = temp_workspace_path("registry-invalid");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = SourcePointer::new(
            "fontes:missing",
            "giodl73-repo/FONTES",
            "fontes:missing",
            "",
            "fontes.missing",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "boundary present",
        );

        let error =
            Workspace::add_source_pointer(&path, pointer).expect_err("invalid pointer should fail");
        let status = Workspace::status(&path).expect("workspace status should load");

        assert_eq!(status.generation, 2);
        assert!(format!("{error}").contains("failure receipt"));
        assert!(path
            .join("receipts")
            .join("registry-add-fontes-missing-0000000002.json")
            .is_file());

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn duplicate_source_pointer_writes_failure_receipt() {
        let path = temp_workspace_path("registry-duplicate");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = valid_pointer("fontes:duplicate");
        Workspace::add_source_pointer(&path, pointer.clone())
            .expect("source pointer should register");

        let error = Workspace::add_source_pointer(&path, pointer)
            .expect_err("duplicate pointer should fail");
        let list = Workspace::list_source_pointers(&path).expect("source pointers should list");

        assert!(format!("{error}").contains("already exists"));
        assert_eq!(list.generation, 3);
        assert_eq!(list.pointers.len(), 1);

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn ingests_registered_pointer_to_file_backed_record() {
        let path = temp_workspace_path("ingest");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = valid_pointer("fontes:apache-calcite:query-planning");
        Workspace::add_source_pointer(&path, pointer).expect("source pointer should register");

        let ingest =
            Workspace::ingest_source_pointer(&path, "fontes:apache-calcite:query-planning")
                .expect("registered source pointer should ingest");

        assert_eq!(ingest.generation, 3);
        assert_eq!(ingest.outcome, "normalized");
        assert_eq!(ingest.grain_count, 10);
        assert_eq!(ingest.bond_count, 20);
        assert_eq!(ingest.receipt_count, 1);
        assert!(ingest.record_path.is_file());
        assert!(ingest.receipt_path.is_file());
        let record_json =
            fs::read_to_string(&ingest.record_path).expect("ingest record should be readable");
        assert!(record_json.contains("\"schema\":\"lattice.ingest-record.v1\""));
        assert!(record_json.contains("\"grains\""));
        assert!(record_json.contains("\"bonds\""));

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn builds_source_corpus_index_from_ingest_records() {
        let path = temp_workspace_path("source-corpus-index");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = valid_pointer("fontes:apache-calcite:query-planning");
        Workspace::add_source_pointer(&path, pointer).expect("source pointer should register");
        Workspace::ingest_source_pointer(&path, "fontes:apache-calcite:query-planning")
            .expect("registered source pointer should ingest");

        let index = Workspace::build_source_corpus_index(&path).expect("index should materialize");

        assert_eq!(index.generation, 4);
        assert_eq!(index.ingest_record_count, 1);
        assert_eq!(index.source_count, 1);
        assert_eq!(index.grain_count, 10);
        assert_eq!(index.bond_count, 20);
        assert_eq!(index.receipt_count, 1);
        assert_eq!(index.index_entry_count, 31);
        assert_eq!(index.artifact_profile, "audit");
        assert_eq!(index.artifact_format, "lattice.source-corpus-index.v1");
        assert!(index.index_path.is_file());
        assert!(index.cache_manifest_path.is_file());
        assert!(index.receipt_path.is_file());
        let index_json = fs::read_to_string(&index.index_path).expect("index should be readable");
        assert!(index_json.contains("\"schema\":\"lattice.source-corpus-index.v1\""));

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn builds_compact_source_corpus_index_profile() {
        let path = temp_workspace_path("source-corpus-index-compact");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = valid_pointer("fontes:apache-calcite:compact");
        Workspace::add_source_pointer(&path, pointer).expect("source pointer should register");
        Workspace::ingest_source_pointer(&path, "fontes:apache-calcite:compact")
            .expect("registered source pointer should ingest");

        let index = Workspace::build_source_corpus_index_with_artifact_profile(
            &path,
            ArtifactProfile::Compact,
        )
        .expect("compact index should materialize");

        assert_eq!(index.artifact_profile, "compact");
        assert_eq!(
            index.artifact_format,
            "lattice.source-corpus-index.compact.v1"
        );
        assert_eq!(
            index.index_path.file_name().and_then(|name| name.to_str()),
            Some("source-corpus-index.compact.json")
        );
        let index_json = fs::read_to_string(&index.index_path).expect("index should be readable");
        assert!(index_json.contains("\"s\":\"lattice.source-corpus-index.compact.v1\""));
        assert!(index_json.contains("\"e\":[["));

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn builds_pebble_source_corpus_index_profile() {
        let path = temp_workspace_path("source-corpus-index-pebble");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = valid_pointer("fontes:apache-calcite:pebble");
        Workspace::add_source_pointer(&path, pointer).expect("source pointer should register");
        Workspace::ingest_source_pointer(&path, "fontes:apache-calcite:pebble")
            .expect("registered source pointer should ingest");

        let index = Workspace::build_source_corpus_index_with_artifact_profile(
            &path,
            ArtifactProfile::Pebble,
        )
        .expect("pebble index should materialize");

        assert_eq!(index.artifact_profile, "pebble");
        assert_eq!(
            index.artifact_format,
            "lattice.pebble-source-corpus-index.v1"
        );
        assert_eq!(
            index.index_path.file_name().and_then(|name| name.to_str()),
            Some("source-corpus-index.pebble.pbl")
        );
        let index_lines = fs::read_to_string(&index.index_path).expect("index should be readable");
        assert!(index_lines.starts_with("PBL1|lattice.pebble-source-corpus-index.v1|"));
        assert!(index_lines.contains("\nS|fontes:apache-calcite:pebble|g10|b20|r1"));

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn ingest_requires_registered_source_pointer() {
        let path = temp_workspace_path("ingest-missing");
        Workspace::init(&path).expect("workspace should initialize");

        let error = Workspace::ingest_source_pointer(&path, "fontes:missing")
            .expect_err("missing source pointer should fail");

        assert!(format!("{error}").contains("not registered"));

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn generation_conflict_writes_failure_receipt() {
        let path = temp_workspace_path("generation-conflict");
        Workspace::init(&path).expect("workspace should initialize");
        let workspace = Workspace::at(&path);

        let error = workspace
            .verify_transaction_generation("registry-add", 0)
            .expect_err("stale generation should conflict");
        let status = Workspace::status(&path).expect("workspace status should load");

        assert!(format!("{error}").contains("generation 0"));
        assert_eq!(status.generation, 2);
        assert!(path
            .join("receipts")
            .join("generation-conflict-registry-add-0000000002.json")
            .is_file());

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn source_pointer_header_is_thirteen_columns() {
        // Pulse-card boundary: the 13-column TSV header is the contract.
        // Any column add/rename here must be coordinated with a wave pulse.
        let column_count = SOURCE_POINTER_HEADER.trim_end().split('\t').count();
        assert_eq!(column_count, 13, "header columns must remain 13");
        let columns = SOURCE_POINTER_HEADER
            .trim_end()
            .split('\t')
            .collect::<Vec<_>>();
        assert_eq!(columns[0], "source_id");
        assert_eq!(columns[8], "rights_boundary");
        assert_eq!(columns[9], "refresh_status");
        assert_eq!(columns[10], "refresh_check");
        assert_eq!(columns[11], "custody_owner");
        assert_eq!(columns[12], "custody_distributor");
    }

    #[test]
    fn source_pointer_round_trip_preserves_all_thirteen_fields() {
        let path = temp_workspace_path("registry-round-trip-13");
        Workspace::init(&path).expect("workspace should initialize");
        let pointer = SourcePointer::with_custody(
            "nist:sp:800-53:rev5",
            "usnistgov/oscal-content",
            "nist:sp:800-53:rev5:catalog",
            "https://raw.githubusercontent.com/usnistgov/oscal-content/main/nist.gov/SP800-53/rev5/json/NIST_SP-800-53_rev5_catalog.json",
            "nist:sp:800-53:rev5:catalog",
            "https://github.com/usnistgov/oscal-content/tree/main/nist.gov/SP800-53/rev5",
            ".proof\\sources\\nist-sp-800-53-rev5.source.md",
            "public_domain_cc0",
            "Public domain in the United States per 17 USC Section 105.",
            "pinned_to_owner_repo_main_branch",
            "Poll usnistgov/oscal-content default branch for new commits.",
            "U.S. National Institute of Standards and Technology (NIST), Information Technology Laboratory",
            "usnistgov/oscal-content (GitHub)",
        );

        Workspace::add_source_pointer(&path, pointer.clone())
            .expect("13-field pointer should register");
        let list = Workspace::list_source_pointers(&path).expect("source pointers should list");

        assert_eq!(list.pointers.len(), 1);
        assert_eq!(list.pointers[0], pointer);
        // Verify the on-disk row actually has 13 tab-separated values plus
        // the header. The header line itself must also have 13 columns.
        let contents = fs::read_to_string(path.join("registry").join("source-pointers.tsv"))
            .expect("registry tsv readable");
        let mut lines = contents.lines();
        let header = lines.next().expect("header present");
        assert_eq!(header.split('\t').count(), 13);
        let row = lines.next().expect("registry row present");
        assert_eq!(
            row.split('\t').count(),
            13,
            "persisted row must have 13 tab-separated values"
        );

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    #[test]
    fn source_pointer_legacy_nine_column_tsv_loads_with_migration_defaults() {
        // Migration contract: legacy workspaces written before the L06
        // follow-on (9-column TSV header + 9-field rows) must continue to
        // load. The four new custody fields are filled with the documented
        // sentinel (SOURCE_POINTER_LEGACY_CUSTODY_FIELD).
        let path = temp_workspace_path("registry-legacy-migration");
        Workspace::init(&path).expect("workspace should initialize");

        let registry_dir = path.join("registry");
        let registry_path = registry_dir.join("source-pointers.tsv");
        let legacy = "source_id\towner_repo\twork_id\tfletch_registry_path\tfletch_id\tproof_ledger_path\tproof_record_path\trights_policy\trights_boundary\n\
fontes:legacy-row\tgiodl73-repo/FONTES\tfontes:legacy-row\t.fletch\\registries\\fontes-legacy.json\tfontes.legacy\tsources\\tables\\proof-source-ledger.json\t.proof\\sources\\fontes-legacy.source.md\tderived_text_allowed\tboundary present\n";
        fs::write(&registry_path, legacy).expect("write legacy tsv");

        let list = Workspace::list_source_pointers(&path).expect("legacy pointers should list");
        assert_eq!(list.pointers.len(), 1);
        let pointer = &list.pointers[0];
        assert_eq!(pointer.source_id, "fontes:legacy-row");
        assert_eq!(pointer.rights_policy, "derived_text_allowed");
        // The four custody fields default to the documented migration
        // sentinel; this is intentional and observable so the gap is
        // not silent.
        assert_eq!(
            pointer.refresh_status,
            lattice_registry::SOURCE_POINTER_LEGACY_CUSTODY_FIELD
        );
        assert_eq!(
            pointer.refresh_check,
            lattice_registry::SOURCE_POINTER_LEGACY_CUSTODY_FIELD
        );
        assert_eq!(
            pointer.custody_owner,
            lattice_registry::SOURCE_POINTER_LEGACY_CUSTODY_FIELD
        );
        assert_eq!(
            pointer.custody_distributor,
            lattice_registry::SOURCE_POINTER_LEGACY_CUSTODY_FIELD
        );

        fs::remove_dir_all(path).expect("temporary workspace should be removable");
    }

    fn valid_pointer(source_id: &str) -> SourcePointer {
        SourcePointer::new(
            source_id,
            "giodl73-repo/FONTES",
            source_id,
            ".fletch\\registries\\fontes-apache-calcite-query-planning-surfaces.json",
            "fontes.apache-calcite.lattice",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "Apache Calcite documentation text is mapped; source artifacts remain boundary-checked.",
        )
    }

    fn temp_workspace_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("lattice-{label}-{nanos}"))
    }
}
