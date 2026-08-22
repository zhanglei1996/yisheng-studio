use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::error::AppError;

/// Publishes generated artifacts without exposing a partially-written target.
/// Staging paths live beside the final path so rename remains atomic on one volume.
pub(crate) struct AtomicArtifactPublisher;

impl AtomicArtifactPublisher {
    pub(crate) fn stage_file(target: &Path) -> Result<PathBuf, AppError> {
        let parent = target
            .parent()
            .ok_or_else(|| AppError::Validation("产物目标缺少父目录".into()))?;
        fs::create_dir_all(parent).map_err(media_error("无法创建产物目录"))?;
        let stem = target
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| AppError::Validation("产物文件名无效".into()))?;
        let extension = target.extension().and_then(|value| value.to_str());
        let staged_name = extension.map_or_else(
            || format!(".{stem}.{}.staging", Uuid::new_v4()),
            |extension| format!(".{stem}.{}.staging.{extension}", Uuid::new_v4()),
        );
        Ok(parent.join(staged_name))
    }

    pub(crate) fn publish_file<F>(staged: &Path, target: &Path, commit: F) -> Result<(), AppError>
    where
        F: FnOnce() -> Result<(), AppError>,
    {
        validate_non_empty_file(staged)?;
        let backup = target.with_extension(format!("{}.backup", Uuid::new_v4()));
        let had_previous = target.is_file();
        if had_previous {
            fs::rename(target, &backup).map_err(media_error("无法暂存旧产物"))?;
        }
        if let Err(error) = fs::rename(staged, target) {
            restore_file(target, &backup, had_previous);
            return Err(AppError::Media(format!("无法原子发布产物：{error}")));
        }
        if let Err(error) = commit() {
            restore_file(target, &backup, had_previous);
            return Err(error);
        }
        if had_previous {
            let _ = fs::remove_file(backup);
        }
        Ok(())
    }

    pub(crate) fn stage_directory(target: &Path) -> Result<StagedDirectory, AppError> {
        let parent = target
            .parent()
            .ok_or_else(|| AppError::Validation("导出目标缺少父目录".into()))?;
        fs::create_dir_all(parent).map_err(media_error("无法创建导出目录"))?;
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| AppError::Validation("导出目录名无效".into()))?;
        cleanup_stale_staging_directories(parent, name)?;
        let path = parent.join(format!(".{name}.{}.staging", Uuid::new_v4()));
        fs::create_dir(&path).map_err(media_error("无法创建导出暂存目录"))?;
        Ok(StagedDirectory {
            path,
            target: target.to_path_buf(),
            committed: false,
        })
    }
}

fn cleanup_stale_staging_directories(parent: &Path, target_name: &str) -> Result<(), AppError> {
    let prefix = format!(".{target_name}.");
    for entry in fs::read_dir(parent).map_err(media_error("无法检查导出暂存目录"))? {
        let entry = entry.map_err(media_error("无法读取导出暂存目录"))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.starts_with(&prefix)
            && file_name.ends_with(".staging")
            && entry.path().is_dir()
        {
            fs::remove_dir_all(entry.path()).map_err(media_error("无法清理上次未完成的导出"))?;
        }
    }
    Ok(())
}

pub(crate) struct StagedDirectory {
    path: PathBuf,
    target: PathBuf,
    committed: bool,
}

impl StagedDirectory {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn commit(mut self) -> Result<PathBuf, AppError> {
        if self.target.exists() {
            return Err(AppError::Validation("导出目标已存在，拒绝覆盖".into()));
        }
        fs::rename(&self.path, &self.target).map_err(media_error("无法原子发布导出目录"))?;
        self.committed = true;
        Ok(self.target.clone())
    }
}

impl Drop for StagedDirectory {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn validate_non_empty_file(path: &Path) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(media_error("暂存产物不存在"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(AppError::Media("暂存产物为空，拒绝发布".into()));
    }
    Ok(())
}

fn restore_file(target: &Path, backup: &Path, had_previous: bool) {
    let _ = fs::remove_file(target);
    if had_previous {
        let _ = fs::rename(backup, target);
    }
}

fn media_error(context: &'static str) -> impl FnOnce(std::io::Error) -> AppError {
    move |error| AppError::Media(format!("{context}：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let path = std::env::temp_dir().join(format!("yisheng-publish-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn failed_commit_restores_previous_file() {
        let root = temp_root();
        let target = root.join("voice.wav");
        fs::write(&target, b"previous").unwrap();
        let staged = AtomicArtifactPublisher::stage_file(&target).unwrap();
        fs::write(&staged, b"next").unwrap();
        let result = AtomicArtifactPublisher::publish_file(&staged, &target, || {
            Err(AppError::Validation("cancelled".into()))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"previous");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_staging_file_is_never_published() {
        let root = temp_root();
        let target = root.join("voice.wav");
        let staged = AtomicArtifactPublisher::stage_file(&target).unwrap();
        fs::write(&staged, []).unwrap();
        assert!(AtomicArtifactPublisher::publish_file(&staged, &target, || Ok(())).is_err());
        assert!(!target.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn staging_file_preserves_the_target_media_extension() {
        let root = temp_root();
        let staged = AtomicArtifactPublisher::stage_file(&root.join("voice.wav")).unwrap();
        assert_eq!(
            staged.extension().and_then(|value| value.to_str()),
            Some("wav")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dropped_staging_directory_removes_partial_export() {
        let root = temp_root();
        let target = root.join("export");
        let staging = AtomicArtifactPublisher::stage_directory(&target).unwrap();
        let staged_path = staging.path().to_path_buf();
        fs::write(staged_path.join("partial.mp4"), b"partial").unwrap();
        drop(staging);
        assert!(!staged_path.exists());
        assert!(!target.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn next_export_cleans_a_stale_staging_directory() {
        let root = temp_root();
        let target = root.join("export");
        let stale = root.join(".export.interrupted.staging");
        fs::create_dir(&stale).unwrap();
        fs::write(stale.join("partial.mp4"), b"partial").unwrap();

        let staging = AtomicArtifactPublisher::stage_directory(&target).unwrap();

        assert!(!stale.exists());
        assert!(staging.path().is_dir());
        drop(staging);
        let _ = fs::remove_dir_all(root);
    }
}
