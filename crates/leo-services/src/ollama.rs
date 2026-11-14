use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Config;

#[cfg(test)]
const GIB: u64 = 1 << 30;
const MEMORY_SHARE: u64 = 900_000_000;
const DISK_SPARE: u64 = 2_000_000_000;
const STOPPED: &str =
    "Ollama stopped answering. Check that the Ollama app is open, then try again.";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub bytes: u64,
    pub memory_gb: u64,
    pub about: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Machine {
    pub memory_bytes: Option<u64>,
    pub free_disk_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    Fits,
    TooBig,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub completed: u64,
    pub total: u64,
    pub status: String,
}

pub fn catalog() -> &'static [Entry] {
    static CATALOG: LazyLock<Vec<Entry>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("ollama_models.json"))
            .expect("the bundled Ollama catalog is valid")
    });
    &CATALOG
}

pub fn fit(entry: &Entry, machine: &Machine) -> Fit {
    let memory = machine
        .memory_bytes
        .is_some_and(|total| entry.memory_gb * MEMORY_SHARE > total);
    let disk = machine
        .free_disk_bytes
        .is_some_and(|free| entry.bytes + DISK_SPARE > free);
    if memory || disk {
        Fit::TooBig
    } else {
        Fit::Fits
    }
}

pub fn recommended<'a>(catalog: &'a [Entry], machine: &Machine) -> Option<&'a Entry> {
    machine.memory_bytes?;
    catalog
        .iter()
        .filter(|entry| fit(entry, machine) == Fit::Fits)
        .max_by_key(|entry| (entry.memory_gb, entry.bytes))
}

pub fn machine() -> Machine {
    Machine {
        memory_bytes: memory(),
        free_disk_bytes: dirs::home_dir().and_then(|home| fs2::available_space(home).ok()),
    }
}

#[cfg(target_os = "macos")]
fn memory() -> Option<u64> {
    let name = std::ffi::CString::new("hw.memsize").ok()?;
    let mut total: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut total as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && total > 0).then_some(total)
}

#[cfg(windows)]
fn memory() -> Option<u64> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut [u64; 8]) -> i32;
    }
    let mut status = [0u64; 8];
    status[0] = std::mem::size_of::<[u64; 8]>() as u64;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
    (ok && status[1] > 0).then_some(status[1])
}

#[cfg(not(any(target_os = "macos", windows)))]
fn memory() -> Option<u64> {
    None
}

pub(crate) fn root(base_url: &str) -> String {
    base_url
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .replace("://localhost", "://127.0.0.1")
}

pub fn view(cfg: &Config) -> Value {
    let url = cfg.provider("ollama").and_then(|p| p.base_url.clone());
    let running = url
        .as_deref()
        .is_some_and(|url| crate::ai::loopback(url) && crate::health::port_open(url));
    let pulled = match (&url, running) {
        (Some(url), true) => crate::config::choice::ollama_models(url),
        _ => Vec::new(),
    };
    let machine = machine();
    let best = recommended(catalog(), &machine).map(|entry| entry.name.as_str());
    json!({
        "installed": running || crate::health::on_path("ollama"),
        "running": running,
        "memory_bytes": machine.memory_bytes,
        "free_disk_bytes": machine.free_disk_bytes,
        "models": catalog().iter().map(|entry| json!({
            "name": entry.name,
            "bytes": entry.bytes,
            "memory_gb": entry.memory_gb,
            "about": entry.about,
            "fits": fit(entry, &machine) == Fit::Fits,
            "recommended": best == Some(entry.name.as_str()),
            "pulled": pulled.contains(&entry.name),
        })).collect::<Vec<_>>(),
        "pulled": pulled,
    })
}

pub fn plain_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if [
        "no space left",
        "not enough space",
        "insufficient space",
        "disk full",
    ]
    .iter()
    .any(|words| lower.contains(words))
    {
        "There isn't enough free disk space for this model. Free up space or choose a smaller model.".into()
    } else if lower.contains("memory") {
        "This computer doesn't have enough memory for this model. Choose a smaller model.".into()
    } else {
        format!(
            "Ollama couldn't download this model: {}",
            raw.chars().take(200).collect::<String>()
        )
    }
}

fn step(line: &str) -> Result<Progress> {
    let value: Value =
        serde_json::from_str(line).context("Ollama sent a reply leo couldn't read. Try again.")?;
    if let Some(error) = value["error"].as_str() {
        bail!(plain_error(error));
    }
    Ok(Progress {
        completed: value["completed"].as_u64().unwrap_or(0),
        total: value["total"].as_u64().unwrap_or(0),
        status: value["status"].as_str().unwrap_or_default().to_string(),
    })
}

pub fn pull(
    base_url: &str,
    model: &str,
    progress: &mut dyn FnMut(Progress),
    stop: &AtomicBool,
) -> Result<()> {
    if !crate::health::port_open(base_url) {
        bail!("Ollama isn't running. Open the Ollama app, then try again.");
    }
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(6 * 60 * 60))
        .build()?
        .post(format!("{}/api/pull", root(base_url)))
        .json(&json!({ "model": model, "stream": true }))
        .send()
        .map_err(|_| anyhow!(STOPPED))?;
    for line in BufReader::new(response).lines() {
        if stop.load(Ordering::Relaxed) {
            bail!("Download cancelled.");
        }
        let line = line.map_err(|_| anyhow!(STOPPED))?;
        if line.trim().is_empty() {
            continue;
        }
        let step = step(&line)?;
        let done = step.status == "success";
        progress(step);
        if done {
            return Ok(());
        }
    }
    bail!("Ollama ended the download before it finished. Try again.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> &'static Entry {
        catalog().iter().find(|entry| entry.name == name).unwrap()
    }

    fn with(memory_gib: u64, free: u64) -> Machine {
        Machine {
            memory_bytes: Some(memory_gib * GIB),
            free_disk_bytes: Some(free),
        }
    }

    #[test]
    fn the_catalog_parses_smallest_first() {
        let names: Vec<_> = catalog().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "qwen3.5:2b",
                "qwen3.5:4b",
                "qwen3.5:9b",
                "gemma4:12b",
                "gemma4:26b"
            ]
        );
        assert!(catalog().windows(2).all(|w| w[0].bytes < w[1].bytes));
        assert!(catalog()
            .iter()
            .all(|e| !e.about.is_empty() && e.memory_gb >= 4));
    }

    #[test]
    fn fit_holds_at_the_edges() {
        let model = entry("qwen3.5:9b");
        let room = model.bytes + DISK_SPARE;
        assert_eq!(fit(model, &with(16, room)), Fit::Fits);
        assert_eq!(fit(model, &with(16, room - 1)), Fit::TooBig);
        let memory = |bytes: u64| Machine {
            memory_bytes: Some(bytes),
            free_disk_bytes: Some(room),
        };
        assert_eq!(fit(model, &memory(14_400_000_000)), Fit::Fits);
        assert_eq!(fit(model, &memory(14_399_999_999)), Fit::TooBig);
        let eight = entry("qwen3.5:4b");
        assert_eq!(fit(eight, &memory(GIB * 78 / 10)), Fit::Fits);
        assert_eq!(fit(eight, &memory(6 * GIB)), Fit::TooBig);
        assert_eq!(fit(entry("qwen3.5:2b"), &memory(6 * GIB)), Fit::Fits);
        for entry in catalog() {
            assert_eq!(fit(entry, &Machine::default()), Fit::Fits);
        }
        let disk_unknown = Machine {
            memory_bytes: Some(8 * GIB),
            free_disk_bytes: None,
        };
        assert_eq!(fit(entry("qwen3.5:4b"), &disk_unknown), Fit::Fits);
        assert_eq!(fit(model, &disk_unknown), Fit::TooBig);
    }

    #[test]
    fn the_largest_fitting_model_is_recommended() {
        let name = |m: Machine| recommended(catalog(), &m).map(|e| e.name.clone());
        assert_eq!(
            name(with(16, 100_000_000_000)).as_deref(),
            Some("gemma4:12b")
        );
        assert_eq!(name(with(16, 9_000_000_000)).as_deref(), Some("qwen3.5:9b"));
        assert_eq!(
            name(with(8, 100_000_000_000)).as_deref(),
            Some("qwen3.5:4b")
        );
        assert_eq!(
            name(with(64, 100_000_000_000)).as_deref(),
            Some("gemma4:26b")
        );
        assert_eq!(
            name(with(6, 100_000_000_000)).as_deref(),
            Some("qwen3.5:2b")
        );
        assert_eq!(name(with(4, 100_000_000_000)), None);
        assert_eq!(name(with(2, 100_000_000_000)), None);
        assert_eq!(name(Machine::default()), None);
    }

    #[test]
    fn progress_lines_are_read_and_errors_said_plainly() {
        assert_eq!(
            step(r#"{"status":"pulling manifest"}"#).unwrap().status,
            "pulling manifest"
        );
        let part = step(r#"{"status":"pulling abc","digest":"sha256:abc","total":5200000000,"completed":2600000000}"#).unwrap();
        assert_eq!((part.completed, part.total), (2_600_000_000, 5_200_000_000));
        assert_eq!(step(r#"{"status":"success"}"#).unwrap().status, "success");
        let disk = step(r#"{"error":"write /x: no space left on device"}"#).unwrap_err();
        assert!(disk
            .to_string()
            .starts_with("There isn't enough free disk space"));
        assert!(step("not json").is_err());
        assert!(
            plain_error("model requires more system memory (20 GiB) than is available")
                .starts_with("This computer doesn't have enough memory")
        );
        assert!(plain_error("Not enough memory").contains("smaller model"));
        assert_eq!(
            plain_error("pull model manifest: file does not exist"),
            "Ollama couldn't download this model: pull model manifest: file does not exist"
        );
        assert_eq!(
            plain_error(&"x".repeat(500)).chars().count(),
            200 + "Ollama couldn't download this model: ".len()
        );
    }

    fn serve(body: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().take(2).flatten() {
                let mut buffer = [0u8; 8192];
                if stream.read(&mut buffer).unwrap_or(0) == 0 {
                    continue;
                }
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n{body}");
                return;
            }
        });
        format!("http://localhost:{port}/v1")
    }

    #[test]
    fn a_pull_reports_progress_until_success_and_stops_on_errors() {
        let url = serve("{\"status\":\"pulling manifest\"}\n{\"status\":\"pulling a\",\"total\":10,\"completed\":4}\n\n{\"status\":\"success\"}\n");
        let mut seen = Vec::new();
        pull(
            &url,
            "qwen3:4b",
            &mut |p| seen.push(p),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!((seen[1].completed, seen[1].total), (4, 10));
        let url = serve("{\"status\":\"pulling a\",\"total\":10,\"completed\":4}\n{\"error\":\"no space left on device\"}\n");
        let error = pull(&url, "qwen3:4b", &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("disk space"));
        let url = serve("{\"status\":\"pulling a\"}\n");
        let error = pull(&url, "qwen3:4b", &mut |_| {}, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(error.to_string(), "Download cancelled.");
        let error = pull(
            "http://127.0.0.1:1/v1",
            "qwen3:4b",
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("isn't running"));
    }

    #[test]
    fn a_slow_trickle_is_cut_by_the_total_client_timeout() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf);
            let _ = socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n");
            for _ in 0..10 {
                if socket.write_all(b"0123456789").is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap()
            .get(format!("http://{addr}/"))
            .send()
            .unwrap();
        assert!(response.bytes().is_err());
    }
}
