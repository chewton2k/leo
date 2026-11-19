use super::*;

const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x02\0\0\0";

fn picture_at(state: &AppState, path: &str, from: &str) -> Response {
    run(get_picture(
        State(state.clone()),
        Query(PictureAt {
            path: path.into(),
            from: from.into(),
        }),
    ))
}

#[test]
fn a_pasted_picture_is_saved_beside_the_notes_and_served_back() {
    let (state, _d, _ids) = state_with(&[]);
    let added = run(put_picture(
        State(state.clone()),
        Path("pasted-1.png".into()),
        axum::body::Body::from(PICTURE.to_vec()),
    ));
    assert_eq!(added.status(), StatusCode::CREATED);
    let path = json_of(added)["path"].as_str().unwrap().to_string();
    assert!(
        path.starts_with("attachments/") && path.ends_with("-pasted-1.png"),
        "{path}"
    );

    let served = picture_at(&state, &path, "cs130");
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(served.headers()[header::CONTENT_TYPE], "image/png");
    let bytes = run(axum::body::to_bytes(served.into_body(), usize::MAX)).unwrap();
    assert_eq!(&bytes[..], PICTURE);

    let notes_dir = state.fresh().notes_dir.clone();
    std::fs::write(notes_dir.join("fake.png"), b"<svg onload=alert(1)>").unwrap();
    for (bad, from) in [
        ("../graph.json", ""),
        ("fake.png", ""),
        ("missing.png", ""),
        (path.as_str(), "../.."),
    ] {
        assert_eq!(
            picture_at(&state, bad, from).status(),
            StatusCode::NOT_FOUND,
            "{bad} from {from}"
        );
    }
    for (name, data) in [("x.svg", &b"<svg/>"[..]), ("x.png", &b"hello"[..])] {
        let refused = run(put_picture(
            State(state.clone()),
            Path(name.into()),
            axum::body::Body::from(data.to_vec()),
        ));
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{name}");
    }
}

fn put_stream(
    state: &AppState,
    name: &str,
    pieces: impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send + 'static,
) -> Response {
    run(put_picture(
        State(state.clone()),
        Path(name.to_string()),
        axum::body::Body::from_stream(pieces),
    ))
}

fn attachments_of(state: &AppState) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(state.fresh().notes_dir.join("attachments"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

#[test]
fn a_large_picture_is_written_as_it_arrives() {
    let (state, _d, _ids) = state_with(&[]);
    let size = 30 * 1024 * 1024;
    let pieces = futures_util::stream::iter((0..size / 4096).map(|n| {
        let mut piece = vec![(n % 251) as u8; 4096];
        if n == 0 {
            piece[..PICTURE.len()].copy_from_slice(PICTURE);
        }
        Ok(piece)
    }));
    let added = put_stream(&state, "scan.png", pieces);
    assert_eq!(added.status(), StatusCode::CREATED);
    let path = json_of(added)["path"].as_str().unwrap().to_string();
    let written = std::fs::read(state.fresh().notes_dir.join(&path)).unwrap();
    assert_eq!(written.len(), size);
    assert_eq!(&written[..PICTURE.len()], PICTURE);
    assert!(written[4096..]
        .chunks(4096)
        .enumerate()
        .all(|(n, c)| c.iter().all(|b| *b == ((n + 1) % 251) as u8)));
    let served = picture_at(&state, &path, "");
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(served.headers()[header::CONTENT_LENGTH], size.to_string());
    let body = run(axum::body::to_bytes(served.into_body(), usize::MAX)).unwrap();
    assert_eq!(body.len(), size);
}

#[test]
fn a_file_that_is_not_a_picture_is_refused_after_its_first_bytes() {
    let (state, _d, _ids) = state_with(&[]);
    use futures_util::StreamExt;
    let later = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let polled = later.clone();
    let pieces = futures_util::stream::iter(0..1000).map(move |n| {
        if n > 0 {
            polled.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(b"<svg onload=alert(1)>".to_vec())
    });
    let refused = put_stream(&state, "x.svg", pieces);
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(later.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(attachments_of(&state).is_empty());
}

#[test]
fn a_picture_that_stops_arriving_leaves_nothing() {
    let (state, _d, _ids) = state_with(&[]);
    let mut first = PICTURE.to_vec();
    first.resize(5000, 1);
    let pieces = futures_util::stream::iter(vec![
        Ok(first),
        Err(std::io::Error::other("the window went away")),
    ]);
    let failed = put_stream(&state, "x.png", pieces);
    assert_ne!(failed.status(), StatusCode::CREATED);
    assert!(attachments_of(&state).is_empty());
}

#[test]
#[ignore]
fn a_big_picture_is_added_in_constant_memory() {
    let (state, _d, _ids) = state_with(&[]);
    let megabytes: usize = std::env::var("PICTURE_MB").unwrap().parse().unwrap();
    let pieces = futures_util::stream::iter((0..megabytes * 16).map(|n| {
        let mut piece = vec![9u8; 64 * 1024];
        if n == 0 {
            piece[..PICTURE.len()].copy_from_slice(PICTURE);
        }
        Ok(piece)
    }));
    assert_eq!(
        put_stream(&state, "big.png", pieces).status(),
        StatusCode::CREATED
    );
}
