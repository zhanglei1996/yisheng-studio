use std::path::{Path, PathBuf};

pub(super) fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if "/:\\".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect()
}

pub(super) fn available_export_directory(output_root: &Path, project_name: &str) -> PathBuf {
    let preferred = output_root.join(project_name);
    if !preferred.exists() {
        return preferred;
    }
    (2..)
        .map(|version| output_root.join(format!("{project_name} ({version})")))
        .find(|candidate| !candidate.exists())
        .expect("export directory version space exhausted")
}

pub(super) fn escape_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

pub(super) fn ffmpeg() -> PathBuf {
    [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
    .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub(super) fn resolve_export_video_source(artifacts: &Path, original: &Path) -> PathBuf {
    let local_proxy = artifacts.join("preview-proxy.mp4");
    if local_proxy.is_file() {
        local_proxy
    } else {
        original.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_prefers_the_durable_local_proxy_after_restart() {
        let root = std::env::temp_dir().join(format!("yisheng-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create artifact directory");
        let original = root.join("outside-source.mov");
        let proxy = root.join("preview-proxy.mp4");
        std::fs::write(&proxy, b"proxy").expect("write proxy");

        assert_eq!(resolve_export_video_source(&root, &original), proxy);
        std::fs::remove_dir_all(root).expect("remove test export root");
    }
}
