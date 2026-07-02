use anyhow::{Context, Result};
use katok::adapters::{FixtureAdapter, KakaocliAdapter, MacosAdapter, SourceAdapter, TxtAdapter};
use std::path::{Path, PathBuf};

pub(super) fn adapter_for_source(
    source: &str,
    path: Option<PathBuf>,
    data_dir: &Path,
) -> Result<Box<dyn SourceAdapter>> {
    match source {
        "fixture" => {
            let fixture_path = path.context("fixture source requires a JSONL path")?;
            Ok(Box::new(FixtureAdapter::new(fixture_path)))
        }
        "txt" => {
            let txt_path = path.context("txt source requires an exported .txt path")?;
            Ok(Box::new(TxtAdapter::new(txt_path)))
        }
        "kakaocli" => Ok(Box::new(KakaocliAdapter)),
        "macos" | "kakao" => {
            let home = katok::kakao::default_home().context("resolve home directory")?;
            Ok(Box::new(MacosAdapter::new(home, data_dir.to_path_buf())))
        }
        other => anyhow::bail!("unsupported source adapter: {other}"),
    }
}
