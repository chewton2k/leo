pub fn allowed(recording: bool) -> bool {
    !recording
}
pub fn play(start: bool) {
    if !allowed(leo_services::speech::engine::engine().recording()) {
        return;
    }
    let Some(_audio) = leo_services::dictation::audio_gate::gate().sound() else {
        return;
    };
    let bytes = if start {
        include_bytes!("../sounds/start.wav").as_slice()
    } else {
        include_bytes!("../sounds/stop.wav").as_slice()
    };
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        let Ok(mut file) = tempfile::NamedTempFile::new() else {
            return;
        };
        if file.write_all(bytes).is_err() {
            return;
        }
        let path = file.path();
        if let Ok(mut child) = leo_core::children::quiet_command("afplay")
            .args(["-v", "0.35"])
            .arg(path)
            .spawn()
        {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(700);
            loop {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows::{
            core::PCWSTR,
            Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_SYNC},
        };
        let _ = PlaySoundW(PCWSTR(bytes.as_ptr().cast()), None, SND_MEMORY | SND_SYNC);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn feedback_never_enters_a_lecture_recording() {
        assert!(!super::allowed(true));
        assert!(super::allowed(false));
    }
}
