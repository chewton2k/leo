pub mod activation;
pub mod audio_gate;
pub mod capture;
pub mod generation;
pub mod pieces;
pub mod polish;
pub mod settings;
pub mod text;
pub mod visualizer;

#[cfg(test)]
mod tests {
    use super::text;
    use super::{
        activation::{Action, Activation},
        settings::{Mode, Settings},
    };
    use std::time::{Duration, Instant};

    #[test]
    fn defaults_are_local_and_filler_removal_is_on() {
        let s = Settings::default();
        assert!(s.fillers);
        assert!(!s.sounds);
        assert_eq!(s.mode, Mode::Both);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn hold_and_tap_both_stop_in_the_right_order() {
        let now = Instant::now();
        let mut a = Activation::default();
        assert_eq!(a.input(true, now, Mode::Both), Action::Start);
        assert_eq!(a.input(true, now, Mode::Both), Action::None);
        assert_eq!(
            a.input(false, now + Duration::from_millis(100), Mode::Both),
            Action::None
        );
        assert_eq!(
            a.input(true, now + Duration::from_secs(1), Mode::Both),
            Action::Stop
        );
        a.reset();
        assert_eq!(a.input(true, now, Mode::Both), Action::Start);
        assert_eq!(
            a.input(false, now + Duration::from_secs(1), Mode::Both),
            Action::Stop
        );
        assert_eq!(
            a.input(false, now + Duration::from_secs(2), Mode::Both),
            Action::None
        );
    }

    #[test]
    fn hold_or_tap_uses_the_reference_300_millisecond_threshold() {
        let mut activation = Activation::default();
        let now = Instant::now();
        assert_eq!(activation.input(true, now, Mode::Both), Action::Start);
        assert_eq!(
            activation.input(false, now + Duration::from_millis(400), Mode::Both),
            Action::Stop
        );
    }
    #[test]
    fn custom_words_then_filler_rules_preserve_unicode() {
        let s = Settings {
            custom_words: vec!["Charlton".into()],
            ..Default::default()
        };
        assert_eq!(
            text::clean("Um, Charlten visits 東京.", &s, Some("en")),
            "Charlton visits 東京."
        );
        assert_eq!(text::clean("um livro", &s, Some("pt")), "um livro");
        let s = Settings {
            fillers: false,
            ..s
        };
        assert_eq!(text::clean("Um, hello.", &s, Some("en")), "Um, hello.");
    }

    #[test]
    fn preferences_are_bounded_and_round_trip_privately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dictation.json");
        let mut s = Settings::default();
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        s.custom_words = vec!["x".repeat(201)];
        assert!(s.save(&path).is_err());
        assert!(serde_json::from_str::<Settings>(r#"{"popup":"unknown"}"#).is_err());
    }
}
