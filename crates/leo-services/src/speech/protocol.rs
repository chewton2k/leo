use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use transcribe_cpp::{
    Backend, Capabilities, DeviceType, RunOptions, StreamOptions, StreamText, StreamUpdate,
    Transcript,
};

const MAX_SECTION_BYTES: usize = 80 * 1024 * 1024;
const PCM_BYTES_PER_MINUTE: usize = 16_000 * 4 * 60;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Hello {
        list_devices: bool,
    },
    Load {
        path: PathBuf,
        backend: Backend,
        device: DeviceSelector,
    },
    Run {
        options: RunOptions,
    },
    StreamBegin {
        run: RunOptions,
        stream: StreamOptions,
    },
    Feed,
    Finalize {
        want_language: bool,
    },
    StreamReset,
    Diarize {
        model: PathBuf,
        parts: Vec<(PathBuf, u64)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceSelector {
    Auto,
    Key(String),
    Index(usize),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Hello {
        devices: Option<Vec<DeviceInfo>>,
    },
    Loaded(LoadedInfo),
    Transcript(Transcript),
    Fed {
        update: StreamUpdate,
        text: Option<StreamText>,
    },
    Finalized {
        update: StreamUpdate,
        text: StreamText,
        language: Option<String>,
    },
    Turns(Vec<Turn>),
    Ok,
    Error(String),
    BackendError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub device_type: DeviceType,
    pub device_id: Option<String>,
    pub memory_total: u64,
    pub index: Option<usize>,
    pub key: String,
}

impl DeviceInfo {
    pub fn from_device(device: &transcribe_cpp::Device) -> Self {
        Self {
            name: device.name.clone(),
            description: device.description.clone(),
            kind: device.kind.clone(),
            device_type: device.device_type,
            device_id: device.device_id.clone(),
            memory_total: device.memory_total,
            index: device.index,
            key: device_key(&device.kind, device.device_id.as_deref(), &device.name),
        }
    }

    pub fn label(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }

    pub fn is_gpu(&self) -> bool {
        matches!(self.device_type, DeviceType::Gpu | DeviceType::Igpu)
    }
}

pub fn device_key(kind: &str, device_id: Option<&str>, name: &str) -> String {
    let (identity_kind, identity) = match device_id {
        Some(device_id) => ("id", device_id),
        None => ("name", name),
    };
    serde_json::to_string(&(kind, identity_kind, identity))
        .expect("transcribe device identity is always JSON serializable")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedInfo {
    pub arch: String,
    pub variant: String,
    pub backend: String,
    pub device: String,
    pub on_gpu: bool,
    pub capabilities: Capabilities,
    pub supports_initial_prompt: bool,
    pub supports_long_form: bool,
    pub supports_vocabulary: bool,
}

pub fn encode_message<T: Serialize>(message: &T, pcm: Option<&[f32]>) -> io::Result<Vec<u8>> {
    let json = serde_json::to_vec(message).map_err(io::Error::other)?;
    check_section_len(json.len())?;
    let pcm = pcm.unwrap_or(&[]);
    if pcm.iter().any(|s| !s.is_finite()) {
        return Err(io::Error::other("Audio contains an invalid sample"));
    }
    if check_section_len(pcm.len() * 4).is_err() {
        return Err(io::Error::other(format!(
            "audio too long for one transcription: {} minutes (limit {} minutes)",
            pcm.len() * 4 / PCM_BYTES_PER_MINUTE,
            MAX_SECTION_BYTES / PCM_BYTES_PER_MINUTE,
        )));
    }
    let mut frame = Vec::with_capacity(8 + json.len() + pcm.len() * 4);
    frame.extend_from_slice(&(json.len() as u32).to_le_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(&((pcm.len() * 4) as u32).to_le_bytes());
    for sample in pcm {
        frame.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(frame)
}

pub fn write_message<T: Serialize>(
    w: &mut impl Write,
    message: &T,
    pcm: Option<&[f32]>,
) -> io::Result<()> {
    w.write_all(&encode_message(message, pcm)?)?;
    w.flush()
}

pub fn read_message<T: for<'de> Deserialize<'de>>(
    r: &mut impl Read,
) -> io::Result<Option<(T, Vec<f32>)>> {
    let mut len = [0u8; 4];
    let mut filled = 0;
    while filled < len.len() {
        match r.read(&mut len[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let json = read_section_body(r, len)?;
    let message = serde_json::from_slice(&json).map_err(io::Error::other)?;
    let pcm_bytes = read_section(r)?;
    if pcm_bytes.len() % 4 != 0 {
        return Err(io::Error::other(
            "PCM section is not a whole number of f32s",
        ));
    }
    let pcm = pcm_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    Ok(Some((message, pcm)))
}

fn read_section(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    read_section_body(r, len)
}

fn read_section_body(r: &mut impl Read, len: [u8; 4]) -> io::Result<Vec<u8>> {
    let len = u32::from_le_bytes(len) as usize;
    check_section_len(len)?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn check_section_len(len: usize) -> io::Result<()> {
    if len > MAX_SECTION_BYTES {
        return Err(io::Error::other(format!("frame section too large: {len}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_message_with_pcm() {
        let mut buf = Vec::new();
        let pcm = [0.0f32, -1.0, 0.5, f32::MIN_POSITIVE];
        write_message(
            &mut buf,
            &Request::Run {
                options: RunOptions::default(),
            },
            Some(&pcm),
        )
        .unwrap();
        let (req, got): (Request, Vec<f32>) = read_message(&mut buf.as_slice()).unwrap().unwrap();
        assert!(matches!(req, Request::Run { .. }));
        assert_eq!(got, pcm);
    }

    #[test]
    fn clean_eof_is_none() {
        let empty: &[u8] = &[];
        assert!(read_message::<Request>(&mut &*empty).unwrap().is_none());
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let mut buf = Vec::new();
        write_message(
            &mut buf,
            &Request::Run {
                options: RunOptions::default(),
            },
            Some(&[0.5f32; 8]),
        )
        .unwrap();
        for cut in [1, 3, 6, buf.len() - 34, buf.len() - 1] {
            let partial = &buf[..cut];
            let err = read_message::<Request>(&mut &*partial).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
        }
    }

    #[test]
    fn section_limit_is_inclusive_and_fits_the_length_prefix() {
        assert!(check_section_len(MAX_SECTION_BYTES).is_ok());
        assert!(check_section_len(MAX_SECTION_BYTES + 1).is_err());
        assert!(MAX_SECTION_BYTES <= u32::MAX as usize);
    }

    #[test]
    fn device_key_matches_legacy_format() {
        assert_eq!(
            device_key("metal", None, "Metal"),
            r#"["metal","name","Metal"]"#
        );
        assert_eq!(
            device_key("vulkan", Some("0000:01:00.0"), "Vulkan0"),
            r#"["vulkan","id","0000:01:00.0"]"#
        );
    }
}
