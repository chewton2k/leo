#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    pub(crate) modified: std::time::SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(unix)]
    file: (u64, u64),
}

impl Stamp {
    pub(crate) fn of(meta: &std::fs::Metadata) -> Option<Stamp> {
        Some(Stamp {
            len: meta.len(),
            modified: meta.modified().ok()?,
            #[cfg(unix)]
            changed: {
                use std::os::unix::fs::MetadataExt;
                (meta.ctime(), meta.ctime_nsec())
            },
            #[cfg(unix)]
            file: {
                use std::os::unix::fs::MetadataExt;
                (meta.ino(), meta.dev())
            },
        })
    }
}
