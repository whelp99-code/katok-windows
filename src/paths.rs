use crate::{Error, Result};
use std::path::PathBuf;

pub fn default_data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or(Error::HomeDirUnavailable)?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("katok"))
    }
    // Windows: %APPDATA%\katok. Linux/other: $XDG_DATA_HOME/katok (~/.local/share/katok).
    #[cfg(not(target_os = "macos"))]
    {
        let base = dirs::data_dir().ok_or(Error::HomeDirUnavailable)?;
        Ok(base.join("katok"))
    }
}

pub fn ensure_private_dir(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(Error::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).map_err(Error::Io)?.permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).map_err(Error::Io)?;
    }
    Ok(())
}
