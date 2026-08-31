use color_eyre::config::HookBuilder;

pub fn install_color_eyre() -> color_eyre::Result<()> {
    HookBuilder::default()
        .add_frame_filter(Box::new(|frames| {
            frames.retain(|frame| {
                let Some(name) = frame.name.as_deref() else {
                    return false;
                };
                name.starts_with("shaide")
            });
        }))
        .install()
}
