use std::path::Path;

pub const DEFAULT_PORT: u16 = 47318;

const FILE: &str = leo_core::paths::own::APP_PORT;

pub fn remembered(config_dir: &Path) -> u16 {
    std::fs::read_to_string(config_dir.join(FILE))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_PORT)
}

pub fn remember(config_dir: &Path, port: u16) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    leo_core::paths::write_atomic(&config_dir.join(FILE), port.to_string())
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_port_is_replaced_and_remembered() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(remembered(dir.path()), DEFAULT_PORT);
        remember(dir.path(), 47322).unwrap();
        assert_eq!(remembered(dir.path()), 47322);
        std::fs::write(dir.path().join("app-port"), "nonsense").unwrap();
        assert_eq!(remembered(dir.path()), DEFAULT_PORT);
    }
}
