use super::*;
use crate::routes::related::{get_reading, put_reading, ReadingBody};

#[test]
fn every_note_keeps_its_reading_place() {
    let (state, _d, _ids) = state_with(&[("Long read", "")]);
    let ids: Vec<String> = (0..2_100).map(|n| format!("note-{n:05}")).collect();
    run(async {
        for (n, id) in ids.iter().enumerate() {
            let anchor = serde_json::json!({"line": n});
            let put = put_reading(
                State(state.clone()),
                Path(id.clone()),
                Json(
                    serde_json::from_value::<ReadingBody>(serde_json::json!({ "anchor": anchor }))
                        .unwrap(),
                ),
            )
            .await;
            assert_eq!(put.status(), 204);
        }
    });
    for (n, id) in ids.iter().enumerate() {
        let got = json_of(run(get_reading(State(state.clone()), Path(id.clone()))));
        assert_eq!(got["anchor"]["line"], n, "the place of {id} was lost");
    }
}
