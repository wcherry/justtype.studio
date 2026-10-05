//! Manuscript content lives on disk, not in SQLite: scene bodies are TipTap
//! JSON documents that are large, mostly-immutable, and never queried by
//! content. SQLite holds the metadata and the relationships between them.
//!
//! Layout under `data_dir`:
//!   <work_id>/working/<scene_id>.json                 mutable draft surface
//!   <work_id>/revisions/<scene_id>/<revision_id>.json autosave history
//!   <work_id>/versions/<version_id>/version.json      snapshot manifest
//!   <work_id>/versions/<version_id>/scenes/<id>.json  frozen snapshot bodies
//!
//! Everything goes through this type, so swapping in object storage later means
//! reimplementing one trait's worth of methods.
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
}

/// Ids come from the URL, so never let one walk out of the data directory.
fn safe_id(id: &str) -> ApiResult<&str> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(id)
    } else {
        Err(ApiError::BadRequest(format!("invalid identifier: {id}")))
    }
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn init(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    fn work_dir(&self, work_id: &str) -> ApiResult<PathBuf> {
        Ok(self.root.join(safe_id(work_id)?))
    }

    pub fn working_scene_path(&self, work_id: &str, scene_id: &str) -> ApiResult<PathBuf> {
        Ok(self
            .work_dir(work_id)?
            .join("working")
            .join(format!("{}.json", safe_id(scene_id)?)))
    }

    pub fn revision_path(
        &self,
        work_id: &str,
        scene_id: &str,
        revision_id: &str,
    ) -> ApiResult<PathBuf> {
        Ok(self
            .work_dir(work_id)?
            .join("revisions")
            .join(safe_id(scene_id)?)
            .join(format!("{}.json", safe_id(revision_id)?)))
    }

    pub fn version_dir(&self, work_id: &str, version_id: &str) -> ApiResult<PathBuf> {
        Ok(self
            .work_dir(work_id)?
            .join("versions")
            .join(safe_id(version_id)?))
    }

    pub fn version_scene_path(
        &self,
        work_id: &str,
        version_id: &str,
        scene_id: &str,
    ) -> ApiResult<PathBuf> {
        Ok(self
            .version_dir(work_id, version_id)?
            .join("scenes")
            .join(format!("{}.json", safe_id(scene_id)?)))
    }

    pub async fn read_json(&self, path: &Path) -> ApiResult<Value> {
        let bytes = tokio::fs::read(path).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ApiError::NotFound("document not found".into()),
            _ => ApiError::Internal(format!("read {}: {e}", path.display())),
        })?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Write-then-rename so a crash mid-save can never leave a half-written scene.
    pub async fn write_json(&self, path: &Path, value: &Value) -> ApiResult<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(value)?;
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }

    pub async fn read_working_scene(&self, work_id: &str, scene_id: &str) -> ApiResult<Value> {
        let path = self.working_scene_path(work_id, scene_id)?;
        match self.read_json(&path).await {
            Ok(v) => Ok(v),
            Err(ApiError::NotFound(_)) => Ok(crate::domain::doc::empty_doc()),
            Err(e) => Err(e),
        }
    }

    pub async fn write_working_scene(
        &self,
        work_id: &str,
        scene_id: &str,
        doc: &Value,
    ) -> ApiResult<()> {
        let path = self.working_scene_path(work_id, scene_id)?;
        self.write_json(&path, doc).await
    }

    pub async fn write_revision(
        &self,
        work_id: &str,
        scene_id: &str,
        revision_id: &str,
        doc: &Value,
    ) -> ApiResult<()> {
        let path = self.revision_path(work_id, scene_id, revision_id)?;
        self.write_json(&path, doc).await
    }

    pub async fn read_revision(
        &self,
        work_id: &str,
        scene_id: &str,
        revision_id: &str,
    ) -> ApiResult<Value> {
        let path = self.revision_path(work_id, scene_id, revision_id)?;
        self.read_json(&path).await
    }

    pub async fn write_version_scene(
        &self,
        work_id: &str,
        version_id: &str,
        scene_id: &str,
        doc: &Value,
    ) -> ApiResult<()> {
        let path = self.version_scene_path(work_id, version_id, scene_id)?;
        self.write_json(&path, doc).await
    }

    pub async fn read_version_scene(
        &self,
        work_id: &str,
        version_id: &str,
        scene_id: &str,
    ) -> ApiResult<Value> {
        let path = self.version_scene_path(work_id, version_id, scene_id)?;
        self.read_json(&path).await
    }

    pub async fn write_version_manifest(
        &self,
        work_id: &str,
        version_id: &str,
        manifest: &Value,
    ) -> ApiResult<()> {
        let path = self.version_dir(work_id, version_id)?.join("version.json");
        self.write_json(&path, manifest).await
    }

    pub async fn delete_scene(&self, work_id: &str, scene_id: &str) -> ApiResult<()> {
        let path = self.working_scene_path(work_id, scene_id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Every file under a work, as paths relative to the work's folder and
    /// their bytes — for handing a writer everything we hold of theirs.
    pub async fn work_files(&self, work_id: &str) -> ApiResult<Vec<(String, Vec<u8>)>> {
        let dir = self.work_dir(work_id)?;
        tokio::task::spawn_blocking(move || {
            fn walk(
                root: &Path,
                dir: &Path,
                out: &mut Vec<(String, Vec<u8>)>,
            ) -> std::io::Result<()> {
                let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
                entries.sort_by_key(|e| e.path());
                for entry in entries {
                    let path = entry.path();
                    if entry.file_type()?.is_dir() {
                        walk(root, &path, out)?;
                    } else if let Ok(relative) = path.strip_prefix(root) {
                        out.push((
                            relative.to_string_lossy().replace('\\', "/"),
                            std::fs::read(&path)?,
                        ));
                    }
                }
                Ok(())
            }
            let mut out = Vec::new();
            match walk(&dir, &dir, &mut out) {
                Ok(()) => Ok(out),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(e.into()),
            }
        })
        .await
        .map_err(|e| crate::error::ApiError::internal(format!("task panicked: {e}")))?
    }

    pub async fn delete_work(&self, work_id: &str) -> ApiResult<()> {
        let dir = self.work_dir(work_id)?;
        match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
