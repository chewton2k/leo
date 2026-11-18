type Streaming = fn(
    leo_services::ai::chat::Prompt,
    u32,
    &mut dyn FnMut(&str),
    &mut dyn FnMut(),
    &dyn Fn() -> bool,
) -> anyhow::Result<(String, Option<leo_services::ai::spend::Answered>)>;

fn streamer_of(run: Streaming) -> leo_web::Streamer {
    std::sync::Arc::new(
        move |system: &str,
              user: &str,
              most: u32,
              piece: &mut dyn FnMut(&str),
              restart: &mut dyn FnMut(),
              stopped: &dyn Fn() -> bool| {
            let (text, answered) = run(
                leo_services::ai::chat::Prompt {
                    system: system.to_string(),
                    user: user.to_string(),
                },
                most,
                piece,
                restart,
                stopped,
            )
            .map_err(crate::felix::said)?;
            Ok(leo_web::Reply {
                text,
                spent: answered.map(crate::felix::spent_of),
            })
        },
    )
}

fn sweep_leftover_temps() {
    let models = leo_services::config::choice::models_dir();
    let folders = [
        leo_core::paths::config_dir().ok(),
        Some(models.join("vad")),
        Some(leo_services::meaning::dir()),
        Some(models.join("speech")),
    ];
    for folder in folders.into_iter().flatten() {
        leo_core::paths::sweep_stale_temps(&folder, 0);
    }
    if let Ok(root) = leo_services::session::root() {
        leo_services::session::sweep_temps(&root);
    }
}

pub fn powers() -> leo_web::Powers {
    std::thread::spawn(|| {
        leo_services::ai::tidy_rooms();
        sweep_leftover_temps();
    });
    let usage = std::sync::Arc::new(leo_web::usage::Usage::default());
    let writer = crate::felix::writer(
        std::sync::Arc::clone(&usage),
        leo_web::usage::Purpose::OtherWriting,
    );
    let streamer = streamer_of(leo_services::ai::chat_streaming);
    let light_chat = streamer_of(leo_services::ai::quick_streaming);
    let toolless_chat = streamer_of(leo_services::ai::toolless_streaming);
    leo_web::Powers {
        writer: Some(writer),
        quick: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::QuickReview,
        )),
        toolless: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::MarginRead,
        )),
        housekeeping: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::Summary,
        )),
        usage,
        chat: Some(streamer),
        light_chat: Some(light_chat),
        lighter: Some(std::sync::Arc::new(leo_services::ai::lighter_model_exists)),
        toolless_chat: Some(toolless_chat),
        dictation: None,
        app_settings: None,
        settings: Some(std::sync::Arc::new(crate::settings::WebSettings)),
        importer: Some(std::sync::Arc::new(
            |files: Vec<leo_web::UploadFile>,
             wants: &str,
             progress: &mut dyn FnMut(&str, usize, usize)| {
                let uploads: Vec<leo_services::import::Upload> = files
                    .into_iter()
                    .map(|f| leo_services::import::Upload {
                        name: f.name,
                        mime: f.mime,
                        bytes: f.bytes,
                        path: f.path,
                    })
                    .collect();
                let made = leo_services::import::import(&uploads, wants, progress)?;
                Ok(leo_web::Made {
                    title: made.title,
                    body: made.body,
                    figures: made
                        .figures
                        .into_iter()
                        .map(|f| {
                            Ok(leo_web::Figure {
                                place: f.place,
                                bytes: std::fs::read(&f.path)?,
                                photo: f.photo,
                            })
                        })
                        .collect::<anyhow::Result<_>>()?,
                })
            },
        )),
        speech: Some(std::sync::Arc::new(crate::speech::Speech)),
        ollama: Some(std::sync::Arc::new(crate::settings::WebSettings)),
        listener: Some(crate::record::listener()),
        regenerator: Some(crate::record::regenerator()),
        calendar_access: Some(std::sync::Arc::new(crate::settings::WebSettings)),
        housekeeper: Some(std::sync::Arc::new(crate::housekeeping::Housekeeping)),
        room: Some(std::sync::Arc::new(leo_services::ai::felix_room)),
        context: Some(std::sync::Arc::new(leo_services::ai::felix_context)),
        reader: Some(std::sync::Arc::new(
            |file: leo_web::UploadFile, progress: &mut dyn FnMut(&str)| {
                leo_services::import::read_for_chat(
                    &leo_services::import::Upload {
                        name: file.name,
                        mime: file.mime,
                        bytes: file.bytes,
                        path: file.path,
                    },
                    &|prompt, images, most| {
                        leo_services::ai::see(prompt, images, most).map_err(crate::felix::said)
                    },
                    progress,
                )
            },
        )),
        reader_to: Some(std::sync::Arc::new(
            |path: &std::path::Path,
             name: &str,
             mime: &str,
             progress: &mut dyn FnMut(&str),
             out: &mut dyn std::io::Write| {
                leo_services::import::read_to(
                    path,
                    name,
                    mime,
                    &|prompt, images, most| {
                        leo_services::ai::see(prompt, images, most).map_err(crate::felix::said)
                    },
                    progress,
                    out,
                )
            },
        )),
        converse: Some(crate::felix::converser()),
        seer: Some(crate::felix::seer()),
        meaning: Some(crate::felix::meaning()),
    }
}
