use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use gpui::{Image, ImageFormat};

static RH: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/brand/rh.png").to_vec(),
    ))
});

static GHOST: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/brand/ghost.png").to_vec(),
    ))
});

pub fn rh() -> Arc<Image> {
    Arc::clone(&RH)
}

pub fn ghost() -> Arc<Image> {
    Arc::clone(&GHOST)
}

pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-400.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-500.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-600.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-700.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf").as_slice()),
    ]
}
