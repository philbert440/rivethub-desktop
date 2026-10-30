use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    ClickEvent, Context, ExternalPaths, ImageFormat, IntoElement, ParentElement, PathPromptOptions, Styled,
    div, prelude::*, px,
};
use model::prompt::{Attachment, image_media_type};

use super::SessionPane;
use crate::theme::Theme;

const IMAGES_ONLY: &str = "Only PNG, JPEG, GIF and WebP images can be sent.";

impl SessionPane {
    fn attach(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach images".into()),
        });
        self.attach_problem = None;
        cx.spawn(async move |this, cx| {
            let paths = match picked.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.attach_problem = Some(format!("couldn't open the file picker: {err}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            this.update(cx, |this, cx| this.attach_paths(paths, cx)).ok();
        })
        .detach();
    }

    /// Clears the last attachment problem, or says why nothing can be attached here.
    fn accepts_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        let takes_images = self.takes_images();
        self.attach_problem = (!takes_images).then(|| {
            format!(
                "{} doesn't take attachments in this session",
                self.summary.id.harness().display_name()
            )
        });
        cx.notify();
        takes_images
    }

    /// Files dropped on the chat attach when rh takes them, which today means images only.
    pub(super) fn attach_dropped(&mut self, dropped: &ExternalPaths, cx: &mut Context<Self>) {
        if !self.accepts_attachments(cx) {
            return;
        }
        let (images, refused): (Vec<PathBuf>, Vec<PathBuf>) =
            dropped.paths().iter().cloned().partition(|path| {
                path.file_name()
                    .and_then(|name| image_media_type(&name.to_string_lossy()))
                    .is_some()
            });
        if !refused.is_empty() {
            let names = refused
                .iter()
                .map(|path| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy())
                .collect::<Vec<_>>()
                .join(", ");
            self.attach_problem = Some(format!("Not attached: {names}. {IMAGES_ONLY}"));
        }
        self.attach_paths(images, cx);
        cx.notify();
    }

    fn attach_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let hub = self.hub.clone();
            let summary = self.summary.clone();
            self.track_upload(async move { hub.upload(&summary, path).await }, cx);
        }
    }

    pub(super) fn attach_pasted(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        if !self.accepts_attachments(cx) {
            return;
        }
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        let name = format!("pasted-image.{}", image.format.extension());
        if image_media_type(&name).is_none() {
            self.attach_problem = Some(IMAGES_ONLY.to_owned());
            cx.notify();
            return;
        }
        let bytes = image.bytes.clone();
        self.track_upload(async move { hub.upload_bytes(&summary, name, bytes).await }, cx);
    }

    /// Counts an upload in flight, so the prompt waits for it, and adds what it uploaded to the next prompt.
    fn track_upload<E: std::fmt::Display>(
        &mut self,
        upload: impl Future<Output = Result<Attachment, E>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.uploading += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let uploaded = upload.await;
            this.update(cx, |this, cx| {
                this.uploading = this.uploading.saturating_sub(1);
                match uploaded {
                    Ok(attachment) => this.attachments.push(attachment),
                    Err(err) => this.attach_problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Fetches the images sent earlier in the session that are still held on the node, once each.
    pub(super) fn fetch_previews(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<(String, ImageFormat)> = self
            .exchanges
            .iter()
            .flat_map(|view| &view.exchange.files)
            .filter_map(|file| {
                let upload = file
                    .viewable_upload()
                    .filter(|upload| !self.requested_previews.contains(*upload))?;
                Some((upload.to_owned(), ImageFormat::from_mime_type(&file.media_type)?))
            })
            .collect();
        for (upload, format) in wanted {
            if !self.requested_previews.insert(upload.clone()) {
                continue;
            }
            let hub = self.hub.clone();
            let summary = self.summary.clone();
            cx.spawn(async move |this, cx| {
                let fetched = hub.download(&summary, upload.clone()).await;
                let bytes = match fetched {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        tracing::debug!(%err, upload, "a sent image is no longer available");
                        return;
                    }
                };
                this.update(cx, |this, cx| {
                    let image = Arc::new(gpui::Image::from_bytes(format, bytes));
                    Arc::make_mut(&mut this.previews).insert(upload.clone(), image);
                    let showing = this.exchanges.iter().enumerate().filter(|(_, view)| {
                        view.exchange
                            .files
                            .iter()
                            .any(|file| file.viewable_upload() == Some(upload.as_str()))
                    });
                    let items = this.turns.item_count();
                    for (index, _) in showing.filter(|(index, _)| *index < items) {
                        this.turns.splice(index..index + 1, 1);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    pub(super) fn attach_button(cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("attach")
            .size(px(34.))
            .rounded(px(17.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.attach(cx)))
            .child(crate::brand::icon("icons/plus.svg", 18., Theme::ink_dim()))
    }

    pub(super) fn attachment_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.attachments.is_empty() && self.uploading == 0 && self.attach_problem.is_none() {
            return None;
        }
        let chip = || {
            div()
                .h(px(28.))
                .px_3()
                .rounded(px(14.))
                .bg(Theme::raised())
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(Theme::ink())
        };
        Some(
            div()
                .px_4()
                .pt_3()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .children(self.attachments.iter().enumerate().map(|(index, attachment)| {
                    chip()
                        .child(div().max_w(px(220.)).truncate().child(attachment.name.clone()))
                        .child(
                            div()
                                .id(("detach", index))
                                .cursor_pointer()
                                .text_color(Theme::ink_dim())
                                .hover(|remove| remove.text_color(Theme::ink()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if index < this.attachments.len() {
                                        this.attachments.remove(index);
                                    }
                                    cx.notify();
                                }))
                                .child("×"),
                        )
                }))
                .when(self.uploading > 0, |row| {
                    row.child(
                        chip()
                            .text_color(Theme::ink_dim())
                            .child(format!("Uploading {}…", self.uploading)),
                    )
                })
                .children(
                    self.attach_problem
                        .clone()
                        .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
                ),
        )
    }
}
