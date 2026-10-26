use gpui::SharedString;

pub fn one_line(title: Option<&str>) -> SharedString {
    let title = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Untitled");
    title.split_whitespace().collect::<Vec<_>>().join(" ").into()
}
