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
