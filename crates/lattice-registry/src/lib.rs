#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use lattice_model::{Bond, BondKind, FixtureTier, Grain, LaunchReadinessFixture, TinyModelFixture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCorpusConfig {
    pub schema: String,
    pub corpus_id: String,
    pub owner_repo: String,
    pub source_root: String,
    pub source_surfaces: SourceCorpusSurfaces,
    pub rights_policy: String,
    pub rights_boundary: String,
    pub refresh_status: String,
    pub check_status: String,
    pub pilot_module_id: String,
    pub required_pointer_fields: Vec<String>,
    pub candidate_bonds: Vec<String>,
    pub pack_profiles: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCorpusSurfaces {
    pub module_records: String,
    pub fletch_registries: String,
}

impl SourceCorpusConfig {
    pub fn read(path: impl AsRef<Path>) -> Result<Self, String> {
        let text = fs::read_to_string(path.as_ref()).map_err(|error| {
            format!(
                "failed to read source-corpus config {}: {error}",
                path.as_ref().display()
            )
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        Ok(Self {
            schema: required_json_string(text, "schema")?,
            corpus_id: required_json_string(text, "corpus_id")?,
            owner_repo: required_json_string(text, "owner_repo")?,
            source_root: required_json_string(text, "source_root")?,
            source_surfaces: SourceCorpusSurfaces {
                module_records: required_nested_json_string(
                    text,
                    "source_surfaces",
                    "module_records",
                )?,
                fletch_registries: required_nested_json_string(
                    text,
                    "source_surfaces",
                    "fletch_registries",
                )?,
            },
            rights_policy: required_json_string(text, "rights_policy")?,
            rights_boundary: required_json_string(text, "rights_boundary")?,
            refresh_status: required_json_string(text, "refresh_status")?,
            check_status: required_json_string(text, "check_status")?,
            pilot_module_id: required_nested_json_string(text, "pilot", "module_id")?,
            required_pointer_fields: required_json_string_array(text, "required_pointer_fields")?,
            candidate_bonds: required_json_string_array(text, "candidate_bonds")?,
            pack_profiles: required_json_string_array(text, "pack_profiles")?,
        })
    }

    pub fn validate_for_module(&self, module_id: &str) -> SourceCorpusConfigValidation {
        let mut errors = Vec::new();
        validate_required_string(&mut errors, "schema", &self.schema);
        validate_required_string(&mut errors, "corpus_id", &self.corpus_id);
        validate_required_string(&mut errors, "owner_repo", &self.owner_repo);
        validate_required_string(&mut errors, "source_root", &self.source_root);
        validate_required_string(
            &mut errors,
            "source_surfaces.module_records",
            &self.source_surfaces.module_records,
        );
        validate_required_string(
            &mut errors,
            "source_surfaces.fletch_registries",
            &self.source_surfaces.fletch_registries,
        );
        validate_required_string(&mut errors, "rights_policy", &self.rights_policy);
        validate_required_string(&mut errors, "rights_boundary", &self.rights_boundary);
        validate_required_string(&mut errors, "refresh_status", &self.refresh_status);
        validate_required_string(&mut errors, "check_status", &self.check_status);
        validate_required_string(&mut errors, "module_id", module_id);
        if self.required_pointer_fields.is_empty() {
            errors.push("required_pointer_fields must not be empty".to_string());
        }
        if self.candidate_bonds.is_empty() {
            errors.push("candidate_bonds must not be empty".to_string());
        }
        if self.pack_profiles.is_empty() {
            errors.push("pack_profiles must not be empty".to_string());
        }
        SourceCorpusConfigValidation { errors }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCorpusConfigValidation {
    pub errors: Vec<String>,
}

impl SourceCorpusConfigValidation {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCorpusRegistryReport {
    pub config_path: PathBuf,
    pub source_root: PathBuf,
    pub corpus_id: String,
    pub owner_repo: String,
    pub module_id: String,
    pub registry_file_count: usize,
    pub module_count: usize,
    pub source_pointer_count: usize,
    pub missing_required_field_count: usize,
    pub custody_partial_count: usize,
    pub receipt_count: usize,
    pub dependency_violation_count: usize,
    pub dry_run: bool,
    pub module_record_path: PathBuf,
    pub fletch_registry_path: PathBuf,
    pub rights_policy: String,
    pub refresh_status: String,
    pub check_status: String,
    pub candidate_bond_count: usize,
    pub pack_profile_count: usize,
}

impl SourceCorpusRegistryReport {
    pub fn passed(&self) -> bool {
        self.missing_required_field_count == 0 && self.dependency_violation_count == 0
    }
}

pub fn import_source_corpus_config(
    config_path: impl AsRef<Path>,
    module_id: &str,
    dependency_violation_count: usize,
) -> Result<SourceCorpusRegistryReport, String> {
    let config_path = config_path.as_ref();
    let config = SourceCorpusConfig::read(config_path)?;
    let validation = config.validate_for_module(module_id);
    let config_parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let source_root = config_parent.join(&config.source_root);
    let module_record_path = resolve_source_surface_path(
        &source_root,
        &config.source_surfaces.module_records,
        module_id,
    );
    let module_record = fs::read_to_string(&module_record_path).unwrap_or_default();
    let fletch_registry_path = resolve_fletch_registry_path(
        &source_root,
        &config.source_surfaces.fletch_registries,
        module_id,
        &module_record,
    );
    let source_pointer_count = count_json_field(&module_record, "source_id");
    let custody_partial_count = module_record
        .matches("\"custody_status\": \"partial\"")
        .count()
        + module_record
            .matches("\"custody_status\":\"partial\"")
            .count();
    let mut missing_required_field_count = validation.errors.len();
    if !module_record_path.exists() {
        missing_required_field_count += 1;
    }
    if !fletch_registry_path.exists() {
        missing_required_field_count += 1;
    }
    if source_pointer_count == 0 {
        missing_required_field_count += 1;
    }

    Ok(SourceCorpusRegistryReport {
        config_path: config_path.to_path_buf(),
        source_root,
        corpus_id: config.corpus_id,
        owner_repo: config.owner_repo,
        module_id: module_id.to_string(),
        registry_file_count: count_source_surface_matches(
            config_parent,
            &config.source_root,
            &config.source_surfaces.fletch_registries,
        ),
        module_count: count_source_surface_matches(
            config_parent,
            &config.source_root,
            &config.source_surfaces.module_records,
        ),
        source_pointer_count,
        missing_required_field_count,
        custody_partial_count,
        receipt_count: 1,
        dependency_violation_count,
        dry_run: true,
        module_record_path,
        fletch_registry_path,
        rights_policy: config.rights_policy,
        refresh_status: config.refresh_status,
        check_status: config.check_status,
        candidate_bond_count: config.candidate_bonds.len(),
        pack_profile_count: config.pack_profiles.len(),
    })
}

pub fn source_pointers_for_module_config(
    config_path: impl AsRef<Path>,
    module_id: &str,
) -> Result<Vec<SourcePointer>, String> {
    let config_path = config_path.as_ref();
    let config = SourceCorpusConfig::read(config_path)?;
    let config_parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let source_root = config_parent.join(&config.source_root);
    let module_record_path = resolve_source_surface_path(
        &source_root,
        &config.source_surfaces.module_records,
        module_id,
    );
    let module_record = fs::read_to_string(&module_record_path).map_err(|error| {
        format!(
            "failed to read module record {}: {error}",
            module_record_path.display()
        )
    })?;
    let fletch_registry_path = resolve_fletch_registry_path(
        &source_root,
        &config.source_surfaces.fletch_registries,
        module_id,
        &module_record,
    );
    let module_record_ref = relative_display_path(&source_root, &module_record_path);
    let fletch_registry_ref = relative_display_path(&source_root, &fletch_registry_path);

    Ok(extract_json_objects_from_array(&module_record, "remap")
        .into_iter()
        .filter_map(|entry| {
            let source_id = extract_json_string(&entry, "source_id")?;
            let proof_record_path = extract_json_string(&entry, "source_record")?;
            let work_id = extract_first_json_array_string(&entry, "current_paths")
                .unwrap_or_else(|| source_id.clone());
            Some(SourcePointer::new(
                source_id.clone(),
                config.owner_repo.clone(),
                work_id,
                fletch_registry_ref.clone(),
                format!("{}:{source_id}", config.corpus_id),
                module_record_ref.clone(),
                proof_record_path,
                config.rights_policy.clone(),
                config.rights_boundary.clone(),
            ))
        })
        .collect())
}

pub fn source_pointers_for_all_source_corpus_config(
    config_path: impl AsRef<Path>,
) -> Result<Vec<SourcePointer>, String> {
    let config_path = config_path.as_ref();
    let config = SourceCorpusConfig::read(config_path)?;
    let config_parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let source_root = config_parent.join(&config.source_root);
    let module_record_paths =
        source_surface_matches(&source_root, &config.source_surfaces.module_records);
    let module_ids = module_ids_from_surface_matches(
        &config.source_surfaces.module_records,
        &module_record_paths,
    );
    let mut pointers = Vec::new();
    for module_id in module_ids {
        pointers.extend(source_pointers_for_module_config(config_path, &module_id)?);
    }
    Ok(pointers)
}

pub fn import_all_source_corpus_config(
    config_path: impl AsRef<Path>,
    dependency_violation_count: usize,
) -> Result<SourceCorpusRegistryReport, String> {
    let config_path = config_path.as_ref();
    let config = SourceCorpusConfig::read(config_path)?;
    let validation = config.validate_for_module("all");
    let config_parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let source_root = config_parent.join(&config.source_root);
    let module_record_paths =
        source_surface_matches(&source_root, &config.source_surfaces.module_records);
    let module_ids = module_ids_from_surface_matches(
        &config.source_surfaces.module_records,
        &module_record_paths,
    );
    let registry_file_count = count_source_surface_matches(
        config_parent,
        &config.source_root,
        &config.source_surfaces.fletch_registries,
    );
    let missing_registry_count = module_record_paths
        .iter()
        .zip(module_ids.iter())
        .map(|(module_record_path, module_id)| {
            let module_record = fs::read_to_string(module_record_path).unwrap_or_default();
            resolve_fletch_registry_path(
                &source_root,
                &config.source_surfaces.fletch_registries,
                module_id,
                &module_record,
            )
        })
        .filter(|path| !path.exists())
        .count();
    let mut source_pointer_count = 0;
    let mut custody_partial_count = 0;
    for module_record_path in &module_record_paths {
        let module_record = fs::read_to_string(module_record_path).unwrap_or_default();
        source_pointer_count += count_json_field(&module_record, "source_id");
        custody_partial_count += module_record
            .matches("\"custody_status\": \"partial\"")
            .count()
            + module_record
                .matches("\"custody_status\":\"partial\"")
                .count();
    }
    let mut missing_required_field_count = validation.errors.len() + missing_registry_count;
    if module_record_paths.is_empty() {
        missing_required_field_count += 1;
    }
    if source_pointer_count == 0 {
        missing_required_field_count += 1;
    }

    Ok(SourceCorpusRegistryReport {
        config_path: config_path.to_path_buf(),
        source_root: source_root.clone(),
        corpus_id: config.corpus_id,
        owner_repo: config.owner_repo,
        module_id: "all".to_string(),
        registry_file_count,
        module_count: module_record_paths.len(),
        source_pointer_count,
        missing_required_field_count,
        custody_partial_count,
        receipt_count: 1,
        dependency_violation_count,
        dry_run: true,
        module_record_path: source_root.join(&config.source_surfaces.module_records),
        fletch_registry_path: source_root.join(&config.source_surfaces.fletch_registries),
        rights_policy: config.rights_policy,
        refresh_status: config.refresh_status,
        check_status: config.check_status,
        candidate_bond_count: config.candidate_bonds.len(),
        pack_profile_count: config.pack_profiles.len(),
    })
}

fn count_source_surface_matches(config_parent: &Path, source_root: &str, pattern: &str) -> usize {
    source_surface_matches(&config_parent.join(source_root), pattern).len()
}

fn source_surface_matches(source_root: &Path, pattern: &str) -> Vec<PathBuf> {
    let pattern_path = Path::new(pattern);
    let Some(file_pattern) = pattern_path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let directory = pattern_path.parent().unwrap_or_else(|| Path::new(""));
    let search_dir = source_root.join(directory);

    let Some((prefix, suffix)) = file_pattern.split_once('*') else {
        let path = search_dir.join(file_pattern);
        return path.exists().then_some(path).into_iter().collect();
    };

    let mut paths = fs::read_dir(search_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| {
            let path = entry.path();
            let matches = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix));
            matches.then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn resolve_source_surface_path(source_root: &Path, pattern: &str, module_id: &str) -> PathBuf {
    source_root.join(pattern.replace('*', module_id))
}

fn resolve_fletch_registry_path(
    source_root: &Path,
    fallback_pattern: &str,
    module_id: &str,
    module_record: &str,
) -> PathBuf {
    extract_json_string(module_record, "fletch_registry")
        .map(|path| source_root.join(path))
        .unwrap_or_else(|| resolve_source_surface_path(source_root, fallback_pattern, module_id))
}

fn module_ids_from_surface_matches(pattern: &str, paths: &[PathBuf]) -> Vec<String> {
    let Some(file_pattern) = Path::new(pattern)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return Vec::new();
    };
    let Some((prefix, suffix)) = file_pattern.split_once('*') else {
        return Vec::new();
    };

    paths
        .iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            Some(name.strip_prefix(prefix)?.strip_suffix(suffix)?.to_string())
        })
        .collect()
}

fn count_json_field(text: &str, field: &str) -> usize {
    text.matches(&format!("\"{field}\"")).count()
}

fn relative_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn validate_required_string(errors: &mut Vec<String>, field: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{field} is required"));
    }
}

fn required_json_string(text: &str, field: &str) -> Result<String, String> {
    extract_json_string(text, field).ok_or_else(|| format!("{field} is required"))
}

fn required_nested_json_string(text: &str, object: &str, field: &str) -> Result<String, String> {
    let object_start = text
        .find(&format!("\"{object}\""))
        .ok_or_else(|| format!("{object}.{field} is required"))?;
    extract_json_string(&text[object_start..], field)
        .ok_or_else(|| format!("{object}.{field} is required"))
}

fn required_json_string_array(text: &str, field: &str) -> Result<Vec<String>, String> {
    extract_json_string_array(text, field).ok_or_else(|| format!("{field} is required"))
}

fn extract_json_string(text: &str, field: &str) -> Option<String> {
    let field_start = text.find(&format!("\"{field}\""))?;
    let after_field = &text[field_start..];
    let colon = after_field.find(':')?;
    let after_colon = after_field[colon + 1..].trim_start();
    let value_start = after_colon.strip_prefix('"')?;
    let value_end = value_start.find('"')?;
    Some(value_start[..value_end].to_string())
}

fn extract_json_string_array(text: &str, field: &str) -> Option<Vec<String>> {
    let field_start = text.find(&format!("\"{field}\""))?;
    let after_field = &text[field_start..];
    let open = after_field.find('[')?;
    let after_open = &after_field[open + 1..];
    let close = after_open.find(']')?;
    let body = &after_open[..close];
    Some(
        body.split(',')
            .filter_map(|item| {
                let item = item.trim();
                let item = item.strip_prefix('"')?.strip_suffix('"')?;
                Some(item.to_string())
            })
            .collect(),
    )
}

fn extract_first_json_array_string(text: &str, field: &str) -> Option<String> {
    extract_json_string_array(text, field)?.into_iter().next()
}

fn extract_json_objects_from_array(text: &str, field: &str) -> Vec<String> {
    let Some(field_start) = text.find(&format!("\"{field}\"")) else {
        return Vec::new();
    };
    let after_field = &text[field_start..];
    let Some(open) = after_field.find('[') else {
        return Vec::new();
    };
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut object_start = None;
    for (index, character) in after_field[open + 1..].char_indices() {
        match character {
            '{' => {
                if depth == 0 {
                    object_start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(start) = object_start.take() {
                        objects
                            .push(after_field[open + 1 + start..open + 1 + index + 1].to_string());
                    }
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    objects
}

/// Documented migration default for the four custody fields added by the
/// Live Search L06 follow-on. Legacy TSV rows (9 columns) read back from
/// disk get this sentinel for `refresh_status`, `refresh_check`,
/// `custody_owner`, and `custody_distributor`. The same sentinel is also
/// used by the back-compat `SourcePointer::new` constructor so legacy
/// call sites (fixtures, synthetic generators, `registry add` flag-driven
/// requests that pre-date the four new flags) keep compiling.
///
/// **Registration boundary:** real registration paths (the `registry pilot`
/// CLI surface in particular) MUST use `SourcePointer::with_custody` and
/// supply real values. This sentinel is the *migration* default, never the
/// *registration* default; the pulse-card boundary forbids silent defaults
/// on the registration path.
pub const SOURCE_POINTER_LEGACY_CUSTODY_FIELD: &str = "unspecified-legacy-row";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePointer {
    pub source_id: String,
    pub owner_repo: String,
    pub work_id: String,
    pub fletch_registry_path: String,
    pub fletch_id: String,
    pub proof_ledger_path: String,
    pub proof_record_path: String,
    pub rights_policy: String,
    pub rights_boundary: String,
    pub refresh_status: String,
    pub refresh_check: String,
    pub custody_owner: String,
    pub custody_distributor: String,
}

impl SourcePointer {
    /// Back-compat 9-field constructor. The four custody fields added by the
    /// Live Search L06 follow-on (`refresh_status`, `refresh_check`,
    /// `custody_owner`, `custody_distributor`) are filled with the documented
    /// migration sentinel [`SOURCE_POINTER_LEGACY_CUSTODY_FIELD`].
    ///
    /// New registration paths should use [`SourcePointer::with_custody`] and
    /// supply real values for the four custody fields. This constructor is
    /// retained so existing fixtures, the synthetic generator pointers, and
    /// the legacy `registry add` flag set keep working.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_id: impl Into<String>,
        owner_repo: impl Into<String>,
        work_id: impl Into<String>,
        fletch_registry_path: impl Into<String>,
        fletch_id: impl Into<String>,
        proof_ledger_path: impl Into<String>,
        proof_record_path: impl Into<String>,
        rights_policy: impl Into<String>,
        rights_boundary: impl Into<String>,
    ) -> Self {
        Self::with_custody(
            source_id,
            owner_repo,
            work_id,
            fletch_registry_path,
            fletch_id,
            proof_ledger_path,
            proof_record_path,
            rights_policy,
            rights_boundary,
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD,
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD,
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD,
            SOURCE_POINTER_LEGACY_CUSTODY_FIELD,
        )
    }

    /// Full 13-field constructor. Registration paths that have real values
    /// for the four custody fields (notably `registry pilot`, which sources
    /// them verbatim from L05's research doc) must use this constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn with_custody(
        source_id: impl Into<String>,
        owner_repo: impl Into<String>,
        work_id: impl Into<String>,
        fletch_registry_path: impl Into<String>,
        fletch_id: impl Into<String>,
        proof_ledger_path: impl Into<String>,
        proof_record_path: impl Into<String>,
        rights_policy: impl Into<String>,
        rights_boundary: impl Into<String>,
        refresh_status: impl Into<String>,
        refresh_check: impl Into<String>,
        custody_owner: impl Into<String>,
        custody_distributor: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            owner_repo: owner_repo.into(),
            work_id: work_id.into(),
            fletch_registry_path: fletch_registry_path.into(),
            fletch_id: fletch_id.into(),
            proof_ledger_path: proof_ledger_path.into(),
            proof_record_path: proof_record_path.into(),
            rights_policy: rights_policy.into(),
            rights_boundary: rights_boundary.into(),
            refresh_status: refresh_status.into(),
            refresh_check: refresh_check.into(),
            custody_owner: custody_owner.into(),
            custody_distributor: custody_distributor.into(),
        }
    }

    pub fn has_required_custody_fields(&self) -> bool {
        self.validate().is_ok()
    }

    pub fn validate(&self) -> SourcePointerValidation {
        let mut errors = Vec::new();
        validate_required(
            &mut errors,
            SourcePointerField::SourceId,
            &self.source_id,
            "source id is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::OwnerRepo,
            &self.owner_repo,
            "owner repo is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::WorkId,
            &self.work_id,
            "owner work id is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::FletchRegistryPath,
            &self.fletch_registry_path,
            "FLETCH registry path is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::FletchId,
            &self.fletch_id,
            "FLETCH id is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::ProofLedgerPath,
            &self.proof_ledger_path,
            "proof ledger path is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::ProofRecordPath,
            &self.proof_record_path,
            "proof record path is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::RightsPolicy,
            &self.rights_policy,
            "rights policy is required",
        );
        validate_required(
            &mut errors,
            SourcePointerField::RightsBoundary,
            &self.rights_boundary,
            "rights boundary is required",
        );

        SourcePointerValidation { errors }
    }
}

fn validate_required(
    errors: &mut Vec<SourcePointerValidationError>,
    field: SourcePointerField,
    value: &str,
    message: &'static str,
) {
    if value.trim().is_empty() {
        errors.push(SourcePointerValidationError { field, message });
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePointerValidation {
    pub errors: Vec<SourcePointerValidationError>,
}

impl SourcePointerValidation {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourcePointerField {
    SourceId,
    OwnerRepo,
    WorkId,
    FletchRegistryPath,
    FletchId,
    ProofLedgerPath,
    ProofRecordPath,
    RightsPolicy,
    RightsBoundary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePointerValidationError {
    pub field: SourcePointerField,
    pub message: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestOutcome {
    Normalized,
    Skipped,
    Rejected,
}

impl IngestOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normalized => "normalized",
            Self::Skipped => "skipped",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestReceipt {
    pub rule: String,
    pub outcome: IngestOutcome,
    pub source_id: String,
    pub note: String,
}

impl IngestReceipt {
    pub fn new(
        rule: impl Into<String>,
        outcome: IngestOutcome,
        source_id: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            rule: rule.into(),
            outcome,
            source_id: source_id.into(),
            note: note.into(),
        }
    }
}

pub const INVARIANT_REGISTRY_RESOLVED: &str = "lattice.registry-resolved";
pub const INVARIANT_INGEST_NORMALIZED: &str = "lattice.ingest-normalized";
pub const INVARIANT_BONDS_VALIDATED: &str = "lattice.bonds-validated";
pub const INVARIANT_DIAGNOSTICS_EMITTED: &str = "lattice.diagnostics-emitted";

pub const PASS_RESOLVE_REGISTRY: &str = "ResolveRegistry";
pub const PASS_NORMALIZE_INGEST: &str = "NormalizeIngest";
pub const PASS_VALIDATE_BONDS: &str = "ValidateBonds";
pub const PASS_EMIT_INGEST_RECEIPT: &str = "EmitIngestReceipt";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceGeneratorKind {
    TinyFixture,
    SmallFixture,
    MediumFixture,
    LaunchReadiness,
    LaunchScale240,
    LaunchScale490,
    LaunchScale990,
    LargeFixture,
}

impl SourceGeneratorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TinyFixture => "tiny_fixture",
            Self::SmallFixture => "small_fixture",
            Self::MediumFixture => "medium_fixture",
            Self::LaunchReadiness => "launch_readiness",
            Self::LaunchScale240 => "launch_scale_240",
            Self::LaunchScale490 => "launch_scale_490",
            Self::LaunchScale990 => "launch_scale_990",
            Self::LargeFixture => "large_fixture",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PassEffect {
    Pure,
    ReadRegistry,
    InsertStore,
    EmitReceipt,
}

impl PassEffect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pure => "pure",
            Self::ReadRegistry => "read:registry",
            Self::InsertStore => "insert:store",
            Self::EmitReceipt => "emit:receipt",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorPassReport {
    pub pass_name: &'static str,
    pub invariant: &'static str,
    pub effect: PassEffect,
    pub passed: bool,
    pub note: String,
}

impl GeneratorPassReport {
    pub fn new(
        pass_name: &'static str,
        invariant: &'static str,
        effect: PassEffect,
        passed: bool,
        note: impl Into<String>,
    ) -> Self {
        Self {
            pass_name,
            invariant,
            effect,
            passed,
            note: note.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorRun {
    pub generator: SourceGeneratorKind,
    pub source_id: String,
    pub record: IngestRecord,
    pub passes: Vec<GeneratorPassReport>,
}

impl GeneratorRun {
    pub fn passed(&self) -> bool {
        self.record.outcome == IngestOutcome::Normalized
            && self.passes.iter().all(|pass| pass.passed)
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestRecord {
    pub source_id: String,
    pub owner_repo: String,
    pub work_id: String,
    pub rights_policy: String,
    pub rights_boundary: String,
    pub outcome: IngestOutcome,
    pub grains: Vec<Grain>,
    pub bonds: Vec<Bond>,
    pub receipts: Vec<IngestReceipt>,
}

impl IngestRecord {
    pub fn tiny_fixture(pointer: &SourcePointer) -> Self {
        Self::tier_fixture(pointer, FixtureTier::Tiny)
    }

    pub fn tier_fixture(pointer: &SourcePointer, tier: FixtureTier) -> Self {
        let validation = pointer.validate();
        if !validation.is_ok() {
            return Self::rejected(
                pointer,
                format!(
                    "source pointer failed {} custody validation check(s)",
                    validation.errors.len()
                ),
            );
        }

        let fixture = TinyModelFixture::from_tier(
            tier,
            pointer.source_id.clone(),
            pointer.rights_policy.clone(),
        );
        Self::normalized(
            pointer,
            fixture.grains,
            fixture.bonds,
            format!("lattice.ingest.{}-fixture", tier.as_str()),
            format!(
                "normalized synthetic {} fixture into grains and bonds",
                tier.as_str()
            ),
        )
    }

    pub fn launch_readiness_fixture(pointer: &SourcePointer) -> Self {
        Self::launch_readiness_fixture_with_generated_per_department(pointer, 45)
    }

    pub fn launch_readiness_fixture_with_generated_per_department(
        pointer: &SourcePointer,
        per_department: usize,
    ) -> Self {
        let validation = pointer.validate();
        if !validation.is_ok() {
            return Self::rejected(
                pointer,
                format!(
                    "source pointer failed {} custody validation check(s)",
                    validation.errors.len()
                ),
            );
        }

        let fixture = LaunchReadinessFixture::parse_with_generated_per_department_for_source(
            pointer.source_id.clone(),
            pointer.rights_policy.clone(),
            per_department,
        );
        let mut bonds = fixture.auto_bonds;
        bonds.extend(
            fixture
                .guidance_bonds
                .into_iter()
                .map(|bond| Bond::new(bond.from, bond.to, BondKind::Requires)),
        );
        Self::normalized(
            pointer,
            fixture.grains,
            bonds,
            "lattice.ingest.launch-readiness-fixture",
            format!(
                "normalized synthetic launch-readiness fixture with {} generated fact(s) per department",
                per_department
            ),
        )
    }

    pub fn skipped(pointer: &SourcePointer, note: impl Into<String>) -> Self {
        Self::empty(pointer, IngestOutcome::Skipped, note)
    }

    pub fn rejected(pointer: &SourcePointer, note: impl Into<String>) -> Self {
        Self::empty(pointer, IngestOutcome::Rejected, note)
    }

    fn empty(pointer: &SourcePointer, outcome: IngestOutcome, note: impl Into<String>) -> Self {
        let note = note.into();
        Self {
            source_id: pointer.source_id.clone(),
            owner_repo: pointer.owner_repo.clone(),
            work_id: pointer.work_id.clone(),
            rights_policy: pointer.rights_policy.clone(),
            rights_boundary: pointer.rights_boundary.clone(),
            outcome,
            grains: Vec::new(),
            bonds: Vec::new(),
            receipts: vec![IngestReceipt::new(
                "lattice.ingest.pointer-validation",
                outcome,
                pointer.source_id.clone(),
                note,
            )],
        }
    }

    fn normalized(
        pointer: &SourcePointer,
        grains: Vec<Grain>,
        bonds: Vec<Bond>,
        rule: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            source_id: pointer.source_id.clone(),
            owner_repo: pointer.owner_repo.clone(),
            work_id: pointer.work_id.clone(),
            rights_policy: pointer.rights_policy.clone(),
            rights_boundary: pointer.rights_boundary.clone(),
            outcome: IngestOutcome::Normalized,
            grains,
            bonds,
            receipts: vec![IngestReceipt::new(
                rule,
                IngestOutcome::Normalized,
                pointer.source_id.clone(),
                note,
            )],
        }
    }

    pub fn grain_count(&self) -> usize {
        self.grains.len()
    }

    pub fn bond_count(&self) -> usize {
        self.bonds.len()
    }
}

pub fn generate_tiny_fixture(pointer: &SourcePointer) -> GeneratorRun {
    generate_tier_fixture(pointer, FixtureTier::Tiny)
}

pub fn generate_tier_fixture(pointer: &SourcePointer, tier: FixtureTier) -> GeneratorRun {
    let kind = match tier {
        FixtureTier::Tiny => SourceGeneratorKind::TinyFixture,
        FixtureTier::Small => SourceGeneratorKind::SmallFixture,
        FixtureTier::Medium => SourceGeneratorKind::MediumFixture,
        FixtureTier::Large => SourceGeneratorKind::LargeFixture,
    };
    generate_record(
        pointer,
        kind,
        IngestRecord::tier_fixture(pointer, tier),
        format!(
            "lowered source pointer into deterministic {} grains and bonds",
            tier.as_str()
        ),
    )
}

pub fn generate_launch_readiness_fixture(pointer: &SourcePointer) -> GeneratorRun {
    generate_launch_readiness_fixture_with_generated_per_department(
        pointer,
        SourceGeneratorKind::LaunchReadiness,
        45,
    )
}

pub fn generate_launch_readiness_fixture_with_generated_per_department(
    pointer: &SourcePointer,
    kind: SourceGeneratorKind,
    per_department: usize,
) -> GeneratorRun {
    generate_record(
        pointer,
        kind,
        IngestRecord::launch_readiness_fixture_with_generated_per_department(
            pointer,
            per_department,
        ),
        format!(
            "lowered launch-readiness fixture with {} generated fact(s) per department",
            per_department
        ),
    )
}

pub fn generate_all_fixture_runs() -> Vec<GeneratorRun> {
    vec![
        generate_tier_fixture(&synthetic_pointer("synthetic:tier:tiny"), FixtureTier::Tiny),
        generate_tier_fixture(
            &synthetic_pointer("synthetic:tier:small"),
            FixtureTier::Small,
        ),
        generate_launch_readiness_fixture(&synthetic_pointer("synthetic:launch-readiness")),
        generate_launch_readiness_fixture_with_generated_per_department(
            &synthetic_pointer("synthetic:launch-scale:240"),
            SourceGeneratorKind::LaunchScale240,
            45,
        ),
        generate_launch_readiness_fixture_with_generated_per_department(
            &synthetic_pointer("synthetic:launch-scale:490"),
            SourceGeneratorKind::LaunchScale490,
            95,
        ),
        generate_launch_readiness_fixture_with_generated_per_department(
            &synthetic_pointer("synthetic:launch-scale:990"),
            SourceGeneratorKind::LaunchScale990,
            195,
        ),
    ]
}

fn synthetic_pointer(source_id: &str) -> SourcePointer {
    SourcePointer::new(
        source_id,
        "giodl73-repo/LATTICE",
        source_id,
        "context\\waves\\2026-07-20-public-core\\WAVE.md",
        source_id,
        "docs\\fixtures.md",
        "docs\\fixtures.md",
        "synthetic_public_demo",
        "Synthetic fixture generated inside LATTICE; no external source content is fetched or vendored.",
    )
}

fn generate_record(
    pointer: &SourcePointer,
    kind: SourceGeneratorKind,
    record: IngestRecord,
    normalized_note: impl Into<String>,
) -> GeneratorRun {
    let validation = pointer.validate();
    let registry_resolved = validation.is_ok();
    let normalized = record.outcome == IngestOutcome::Normalized;
    let bonds_valid = normalized
        && record.bonds.iter().all(|bond| {
            record.grains.iter().any(|grain| grain.id == bond.from)
                && record.grains.iter().any(|grain| grain.id == bond.to)
        });
    let receipt_emitted = !record.receipts.is_empty()
        && record
            .receipts
            .iter()
            .all(|receipt| receipt.source_id == pointer.source_id);

    GeneratorRun {
        generator: kind,
        source_id: pointer.source_id.clone(),
        record,
        passes: vec![
            GeneratorPassReport::new(
                PASS_RESOLVE_REGISTRY,
                INVARIANT_REGISTRY_RESOLVED,
                PassEffect::ReadRegistry,
                registry_resolved,
                if registry_resolved {
                    "source pointer custody fields are present".to_string()
                } else {
                    format!(
                        "source pointer failed {} custody validation check(s)",
                        validation.errors.len()
                    )
                },
            ),
            GeneratorPassReport::new(
                PASS_NORMALIZE_INGEST,
                INVARIANT_INGEST_NORMALIZED,
                PassEffect::InsertStore,
                normalized,
                if normalized {
                    normalized_note.into()
                } else {
                    "normalization refused because registry resolution failed".to_string()
                },
            ),
            GeneratorPassReport::new(
                PASS_VALIDATE_BONDS,
                INVARIANT_BONDS_VALIDATED,
                PassEffect::Pure,
                bonds_valid,
                if bonds_valid {
                    "all generated bond endpoints resolve inside the ingest record"
                } else {
                    "bond validation did not run over a normalized ingest record"
                },
            ),
            GeneratorPassReport::new(
                PASS_EMIT_INGEST_RECEIPT,
                INVARIANT_DIAGNOSTICS_EMITTED,
                PassEffect::EmitReceipt,
                receipt_emitted,
                "ingest receipt records the generator outcome",
            ),
        ],
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IngestSummary {
    pub source_pointer_count: usize,
    pub ingest_record_count: usize,
    pub grain_count: usize,
    pub bond_count: usize,
    pub receipt_count: usize,
    pub rejected_record_count: usize,
    pub skipped_record_count: usize,
}

impl IngestSummary {
    pub fn from_records(source_pointer_count: usize, records: &[IngestRecord]) -> Self {
        Self {
            source_pointer_count,
            ingest_record_count: records.len(),
            grain_count: records.iter().map(IngestRecord::grain_count).sum(),
            bond_count: records.iter().map(IngestRecord::bond_count).sum(),
            receipt_count: records.iter().map(|record| record.receipts.len()).sum(),
            rejected_record_count: records
                .iter()
                .filter(|record| record.outcome == IngestOutcome::Rejected)
                .count(),
            skipped_record_count: records
                .iter()
                .filter(|record| record.outcome == IngestOutcome::Skipped)
                .count(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Registry {
    pointers: Vec<SourcePointer>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, pointer: SourcePointer) {
        self.pointers.push(pointer);
    }

    pub fn pointers(&self) -> &[SourcePointer] {
        &self.pointers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn source_corpus_import_uses_configured_source_surface_patterns() {
        let root = test_dir("surface-patterns");
        fs::create_dir_all(root.join("modules")).unwrap();
        fs::create_dir_all(root.join("registries")).unwrap();
        fs::write(
            root.join("modules").join("alpha.module.json"),
            "{\"source_id\":\"example:alpha\",\"custody_status\":\"partial\",\"distribution\":{\"fletch_registry\":\"registries/registry-alpha.json\"}}",
        )
        .unwrap();
        fs::write(root.join("registries").join("registry-alpha.json"), "{}").unwrap();
        fs::write(
            root.join("lattice.source-corpus.json"),
            source_corpus_config_text(),
        )
        .unwrap();

        let report =
            import_source_corpus_config(root.join("lattice.source-corpus.json"), "alpha", 0)
                .unwrap();

        assert!(report.passed());
        assert_eq!(report.module_count, 1);
        assert_eq!(report.registry_file_count, 1);
        assert_eq!(report.source_pointer_count, 1);
        assert!(report
            .fletch_registry_path
            .ends_with(Path::new("registries").join("registry-alpha.json")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_source_corpus_import_sums_configured_modules_without_materializing() {
        let root = test_dir("all-source-corpus");
        fs::create_dir_all(root.join("modules")).unwrap();
        fs::create_dir_all(root.join("registries")).unwrap();
        fs::write(
            root.join("modules").join("alpha.module.json"),
            "{\"source_id\":\"example:alpha:one\",\"custody_status\":\"partial\",\"distribution\":{\"fletch_registry\":\"registries/registry-alpha.json\"}}",
        )
        .unwrap();
        fs::write(
            root.join("modules").join("beta.module.json"),
            "{\"source_id\":\"example:beta:one\",\"source_id\":\"example:beta:two\",\"custody_status\":\"partial\",\"distribution\":{\"fletch_registry\":\"registries/registry-beta.json\"}}",
        )
        .unwrap();
        fs::write(root.join("registries").join("registry-alpha.json"), "{}").unwrap();
        fs::write(root.join("registries").join("registry-beta.json"), "{}").unwrap();
        fs::write(
            root.join("lattice.source-corpus.json"),
            source_corpus_config_text(),
        )
        .unwrap();

        let report =
            import_all_source_corpus_config(root.join("lattice.source-corpus.json"), 0).unwrap();

        assert!(report.passed());
        assert_eq!(report.module_id, "all");
        assert_eq!(report.module_count, 2);
        assert_eq!(report.registry_file_count, 2);
        assert_eq!(report.source_pointer_count, 3);
        assert_eq!(report.custody_partial_count, 2);
        assert!(report.dry_run);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn module_config_lowers_remap_entries_to_source_pointers() {
        let root = test_dir("module-source-pointers");
        fs::create_dir_all(root.join("modules")).unwrap();
        fs::create_dir_all(root.join("registries")).unwrap();
        fs::write(
            root.join("modules").join("alpha.module.json"),
            "{\"remap\":[{\"source_id\":\"example:alpha:one\",\"source_record\":\"sources/one.source-record.md\",\"current_paths\":[\"alpha/ONE.md\"],\"custody_status\":\"partial\"}],\"distribution\":{\"fletch_registry\":\"registries/registry-alpha.json\"}}",
        )
        .unwrap();
        fs::write(root.join("registries").join("registry-alpha.json"), "{}").unwrap();
        fs::write(
            root.join("lattice.source-corpus.json"),
            source_corpus_config_text(),
        )
        .unwrap();

        let pointers =
            source_pointers_for_module_config(root.join("lattice.source-corpus.json"), "alpha")
                .unwrap();

        assert_eq!(pointers.len(), 1);
        assert_eq!(pointers[0].source_id, "example:alpha:one");
        assert_eq!(pointers[0].work_id, "alpha/ONE.md");
        assert_eq!(
            pointers[0].proof_record_path,
            "sources/one.source-record.md"
        );
        assert!(pointers[0].has_required_custody_fields());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_module_configs_lower_remap_entries_to_source_pointers() {
        let root = test_dir("all-module-source-pointers");
        fs::create_dir_all(root.join("modules")).unwrap();
        fs::create_dir_all(root.join("registries")).unwrap();
        fs::write(
            root.join("modules").join("alpha.module.json"),
            "{\"remap\":[{\"source_id\":\"example:alpha:one\",\"source_record\":\"sources/alpha-one.source-record.md\",\"current_paths\":[\"alpha/ONE.md\"],\"custody_status\":\"partial\"}],\"distribution\":{\"fletch_registry\":\"registries/registry-alpha.json\"}}",
        )
        .unwrap();
        fs::write(
            root.join("modules").join("beta.module.json"),
            "{\"remap\":[{\"source_id\":\"example:beta:one\",\"source_record\":\"sources/beta-one.source-record.md\",\"current_paths\":[\"beta/ONE.md\"],\"custody_status\":\"partial\"},{\"source_id\":\"example:beta:two\",\"source_record\":\"sources/beta-two.source-record.md\",\"current_paths\":[\"beta/TWO.md\"],\"custody_status\":\"partial\"}],\"distribution\":{\"fletch_registry\":\"registries/registry-beta.json\"}}",
        )
        .unwrap();
        fs::write(root.join("registries").join("registry-alpha.json"), "{}").unwrap();
        fs::write(root.join("registries").join("registry-beta.json"), "{}").unwrap();
        fs::write(
            root.join("lattice.source-corpus.json"),
            source_corpus_config_text(),
        )
        .unwrap();

        let pointers =
            source_pointers_for_all_source_corpus_config(root.join("lattice.source-corpus.json"))
                .unwrap();

        assert_eq!(pointers.len(), 3);
        assert_eq!(pointers[0].source_id, "example:alpha:one");
        assert_eq!(pointers[1].source_id, "example:beta:one");
        assert_eq!(pointers[2].source_id, "example:beta:two");
        assert!(pointers
            .iter()
            .all(SourcePointer::has_required_custody_fields));

        fs::remove_dir_all(root).unwrap();
    }

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lattice-{name}-{unique}"))
    }

    fn source_corpus_config_text() -> &'static str {
        r#"{
  "schema": "lattice.source_corpus.v1",
  "corpus_id": "example-corpus",
  "owner_repo": "example/repo",
  "source_root": ".",
  "rights_policy": "cc-by-sa-4.0",
  "rights_boundary": "derived metadata only",
  "refresh_status": "manual",
  "check_status": "reviewed-clean",
  "pilot": { "module_id": "alpha" },
  "source_surfaces": {
    "module_records": "modules/*.module.json",
    "fletch_registries": "registries/*.json"
  },
  "required_pointer_fields": ["owner_repo", "module_id"],
  "candidate_bonds": ["belongs_to_module"],
  "pack_profiles": ["answer-grounding"]
}"#
    }

    #[test]
    fn registry_keeps_source_pointer_without_vendoring_content() {
        let mut registry = Registry::new();

        registry.register(SourcePointer::new(
            "fontes:apache-calcite:query-planning",
            "giodl73-repo/FONTES",
            "fontes:apache-calcite:query-planning",
            ".fletch\\registries\\fontes-apache-calcite-query-planning-surfaces.json",
            "fontes.apache-calcite.lattice",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "Apache Calcite documentation text is mapped for derived text; source files and release artifacts remain boundary-checked.",
        ));

        assert_eq!(registry.pointers().len(), 1);
        let pointer = &registry.pointers()[0];
        assert_eq!(pointer.fletch_id, "fontes.apache-calcite.lattice");
        assert_eq!(pointer.rights_policy, "derived_text_allowed");
        assert!(pointer.rights_boundary.contains("boundary-checked"));
        assert!(pointer.has_required_custody_fields());
    }

    #[test]
    fn source_pointer_contract_requires_all_custody_fields() {
        let pointer = SourcePointer::new(
            "fontes:mit:ocw:18-06-linear-algebra-spring-2010",
            "giodl73-repo/FONTES",
            "fontes:mit:ocw:18-06-linear-algebra-spring-2010",
            ".fletch\\registries\\fontes-mit-18-06-surfaces.json",
            "fontes.mit.ocw.18-06.course-page",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "OCW materials cleared; assigned textbooks stay outside OCW license unless separately cleared.",
        );

        assert!(pointer.has_required_custody_fields());
        assert_eq!(pointer.owner_repo, "giodl73-repo/FONTES");
        assert_eq!(
            pointer.fletch_registry_path,
            ".fletch\\registries\\fontes-mit-18-06-surfaces.json"
        );
        assert_eq!(
            pointer.proof_ledger_path,
            "sources\\tables\\proof-source-ledger.json"
        );
    }

    #[test]
    fn source_pointer_contract_rejects_missing_custody_fields() {
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

        assert!(!pointer.has_required_custody_fields());
        let validation = pointer.validate();
        assert_eq!(validation.errors.len(), 1);
        assert_eq!(
            validation.errors[0].field,
            SourcePointerField::FletchRegistryPath
        );
    }

    #[test]
    fn tiny_fixture_ingest_lowers_pointer_to_grains_bonds_and_receipt() {
        let pointer = SourcePointer::new(
            "fontes:apache-calcite:query-planning",
            "giodl73-repo/FONTES",
            "fontes:apache-calcite:query-planning",
            ".fletch\\registries\\fontes-apache-calcite-query-planning-surfaces.json",
            "fontes.apache-calcite.lattice",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "Apache Calcite documentation text is mapped for derived text; source files and release artifacts remain boundary-checked.",
        );

        let record = IngestRecord::tiny_fixture(&pointer);

        assert_eq!(record.outcome, IngestOutcome::Normalized);
        assert_eq!(record.grain_count(), 10);
        assert_eq!(record.bond_count(), 20);
        assert_eq!(record.receipts.len(), 1);
        assert_eq!(record.receipts[0].outcome.as_str(), "normalized");
        assert_eq!(record.rights_policy, "derived_text_allowed");
        assert!(record.rights_boundary.contains("boundary-checked"));
    }

    #[test]
    fn invalid_pointer_ingest_is_rejected_not_success_shaped() {
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

        let record = IngestRecord::tiny_fixture(&pointer);

        assert_eq!(record.outcome, IngestOutcome::Rejected);
        assert_eq!(record.grain_count(), 0);
        assert_eq!(record.bond_count(), 0);
        assert_eq!(record.receipts[0].outcome, IngestOutcome::Rejected);
        assert!(record.receipts[0].note.contains("custody validation"));
    }

    #[test]
    fn skipped_ingest_keeps_audit_receipt() {
        let pointer = SourcePointer::new(
            "fontes:skip",
            "giodl73-repo/FONTES",
            "fontes:skip",
            ".fletch\\registries\\fontes-skip.json",
            "fontes.skip",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "boundary present",
        );

        let record = IngestRecord::skipped(&pointer, "fixture intentionally skipped");

        assert_eq!(record.outcome, IngestOutcome::Skipped);
        assert_eq!(record.receipts[0].outcome.as_str(), "skipped");
        assert!(record.receipts[0].note.contains("intentionally skipped"));
    }

    #[test]
    fn ingest_summary_counts_generator_fixture_outputs() {
        let normalized_pointer = SourcePointer::new(
            "fontes:ok",
            "giodl73-repo/FONTES",
            "fontes:ok",
            ".fletch\\registries\\fontes-ok.json",
            "fontes.ok",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "boundary present",
        );
        let rejected_pointer = SourcePointer::new(
            "fontes:bad",
            "giodl73-repo/FONTES",
            "fontes:bad",
            "",
            "fontes.bad",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "boundary present",
        );
        let skipped_pointer = SourcePointer::new(
            "fontes:skip",
            "giodl73-repo/FONTES",
            "fontes:skip",
            ".fletch\\registries\\fontes-skip.json",
            "fontes.skip",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "boundary present",
        );
        let records = vec![
            IngestRecord::tiny_fixture(&normalized_pointer),
            IngestRecord::tiny_fixture(&rejected_pointer),
            IngestRecord::skipped(&skipped_pointer, "fixture intentionally skipped"),
        ];

        let summary = IngestSummary::from_records(3, &records);

        assert_eq!(summary.source_pointer_count, 3);
        assert_eq!(summary.ingest_record_count, 3);
        assert_eq!(summary.grain_count, 10);
        assert_eq!(summary.bond_count, 20);
        assert_eq!(summary.receipt_count, 3);
        assert_eq!(summary.rejected_record_count, 1);
        assert_eq!(summary.skipped_record_count, 1);
    }

    #[test]
    fn tiny_fixture_generator_reports_canonical_passes() {
        let pointer = SourcePointer::new(
            "fontes:apache-calcite:query-planning",
            "giodl73-repo/FONTES",
            "fontes:apache-calcite:query-planning",
            ".fletch\\registries\\fontes-apache-calcite-query-planning-surfaces.json",
            "fontes.apache-calcite.lattice",
            "sources\\tables\\proof-source-ledger.json",
            ".proof\\sources\\fontes-course-source-ledger.source.md",
            "derived_text_allowed",
            "Apache Calcite documentation text is mapped for derived text; source files and release artifacts remain boundary-checked.",
        );

        let run = generate_tiny_fixture(&pointer);

        assert!(run.passed());
        assert_eq!(run.generator.as_str(), "tiny_fixture");
        assert_eq!(run.pass_count(), 4);
        assert_eq!(run.passes[0].pass_name, PASS_RESOLVE_REGISTRY);
        assert_eq!(run.passes[1].pass_name, PASS_NORMALIZE_INGEST);
        assert_eq!(run.passes[2].pass_name, PASS_VALIDATE_BONDS);
        assert_eq!(run.passes[3].pass_name, PASS_EMIT_INGEST_RECEIPT);
        assert_eq!(run.passes[1].effect.as_str(), "insert:store");
        assert_eq!(run.record.grain_count(), 10);
        assert_eq!(run.record.bond_count(), 20);
        assert_eq!(run.record.rights_policy, "derived_text_allowed");
    }

    #[test]
    fn all_current_synthetic_fixtures_report_generator_passes() {
        let runs = generate_all_fixture_runs();

        assert_eq!(runs.len(), 6);
        assert!(runs.iter().all(GeneratorRun::passed));
        assert_eq!(runs[0].generator, SourceGeneratorKind::TinyFixture);
        assert_eq!(runs[1].generator, SourceGeneratorKind::SmallFixture);
        assert_eq!(runs[2].generator, SourceGeneratorKind::LaunchReadiness);
        assert_eq!(runs[3].generator, SourceGeneratorKind::LaunchScale240);
        assert_eq!(runs[4].generator, SourceGeneratorKind::LaunchScale490);
        assert_eq!(runs[5].generator, SourceGeneratorKind::LaunchScale990);
        assert_eq!(
            runs[1].record.grain_count(),
            FixtureTier::Small.target_grain_count()
        );
        assert_eq!(runs[3].record.grain_count(), 240);
        assert_eq!(runs[4].record.grain_count(), 490);
        assert_eq!(runs[5].record.grain_count(), 990);
        assert!(runs.iter().all(|run| run
            .record
            .grains
            .iter()
            .all(|grain| grain.source_id.as_deref() == Some(run.record.source_id.as_str()))));
    }

    #[test]
    fn tiny_fixture_generator_rejects_invalid_pointer_without_success_shape() {
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

        let run = generate_tiny_fixture(&pointer);

        assert!(!run.passed());
        assert_eq!(run.record.outcome, IngestOutcome::Rejected);
        assert_eq!(run.record.grain_count(), 0);
        assert_eq!(run.record.bond_count(), 0);
        assert_eq!(run.record.receipts.len(), 1);
        assert!(!run.passes[0].passed);
        assert!(!run.passes[1].passed);
        assert!(!run.passes[2].passed);
        assert!(run.passes[3].passed);
    }
}
