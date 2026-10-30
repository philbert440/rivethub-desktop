use std::sync::Arc;
use std::time::Instant;

use gpui::{Context, ImageFormat, prelude::*};
use model::plugin::view::{Block, Button, CanvasBlock, Field, View};

use super::{FormState, Frame, FrameEvent, Picture, child_key};
use crate::plugin::canvas::{CanvasTarget, CanvasView, Replied};
use crate::plugin::form::{FormSubmitted, FormView};

/// What a view holds that the frame keeps between fetches.
#[derive(Default)]
struct Found<'a> {
    forms: Vec<(&'a str, &'a [Field], &'a [Button])>,
    canvases: Vec<&'a CanvasBlock>,
    images: Vec<&'a str>,
    logs: Vec<String>,
}

fn gather<'a>(blocks: &'a [Block], key: &str, found: &mut Found<'a>) {
    for (index, block) in blocks.iter().enumerate() {
        let key = child_key(key, index);
        match block {
            Block::Section { blocks, .. } => gather(blocks, &key, found),
            Block::Split {
                primary, secondary, ..
            } => {
                gather(primary, &format!("{key}p"), found);
                gather(secondary, &format!("{key}s"), found);
            }
            Block::Tabs { tabs, .. } => {
                for tab in tabs {
                    gather(&tab.blocks, &format!("{key}:{}", tab.id), found);
                }
            }
            Block::Form(form) => found.forms.push((&form.id, &form.fields, &form.buttons)),
            Block::Canvas(canvas) => found.canvases.push(canvas),
            Block::Image { path, .. } => found.images.push(path),
            Block::Code { follow: true, .. } => found.logs.push(key),
            _ => {}
        }
    }
}

impl Frame {
    /// Brings the frame's forms, canvases and images in line with a view fetched as `asked`.
    pub(super) fn adopt(&mut self, view: &View, asked: Instant, cx: &mut Context<Self>) {
        let mut found = Found::default();
        gather(&view.blocks, "", &mut found);
        self.forms
            .retain(|id, _| found.forms.iter().any(|(form, ..)| form == id));
        for (id, fields, buttons) in found.forms {
            let unchanged = self
                .forms
                .get(id)
                .is_some_and(|state| state.view.read(cx).given() == fields);
            if let Some(state) = self.forms.get_mut(id).filter(|_| unchanged) {
                state.buttons = buttons.to_vec();
                continue;
            }
            let form = cx.new(|cx| FormView::new(fields.to_vec(), &[], cx));
            let named = id.to_owned();
            let submitted = cx.subscribe(&form, move |this, _, _: &FormSubmitted, cx| {
                this.submit(&named, cx);
            });
            self.forms.insert(
                id.to_owned(),
                FormState {
                    view: form,
                    refusal: None,
                    buttons: buttons.to_vec(),
                    _submitted: submitted,
                },
            );
        }
        self.canvases
            .retain(|id, _| found.canvases.iter().any(|canvas| &canvas.id == id));
        for block in found.canvases {
            let target = CanvasTarget {
                plugin: self.plugin.clone(),
                canvas: block.id.clone(),
                params: self.params.clone(),
                context: self.context.clone(),
            };
            if let Some((canvas, _)) = self.canvases.get(&block.id) {
                canvas.update(cx, |canvas, cx| canvas.renew(block, target, asked, cx));
                continue;
            }
            let hub = self.hub.clone();
            let canvas = cx.new(|cx| CanvasView::new(hub, block, target, cx));
            let replied = cx.subscribe(&canvas, |_, _, Replied(actions), cx| {
                cx.emit(FrameEvent::Run(actions.clone()));
            });
            self.canvases.insert(block.id.clone(), (canvas, replied));
        }
        for path in found.images {
            if !self.pictures.contains_key(path) {
                self.fetch_picture(path.to_owned(), cx);
            }
        }
        for key in found.logs {
            self.logs.entry(key).or_default().scroll_to_bottom();
        }
    }

    fn fetch_picture(&mut self, path: String, cx: &mut Context<Self>) {
        self.pictures.insert(path.clone(), Picture::Loading);
        let hub = self.hub.clone();
        let plugin = self.plugin.clone();
        cx.spawn(async move |this, cx| {
            let fetched = hub.plugin_file(plugin, path.clone()).await;
            this.update(cx, |this, cx| {
                let format = |media_type: Option<&str>| media_type.and_then(ImageFormat::from_mime_type);
                let picture = match fetched {
                    Ok((bytes, media_type)) => format(media_type.as_deref())
                        .map_or(Picture::Failed, |format| {
                            Picture::Ready(Arc::new(gpui::Image::from_bytes(format, bytes)))
                        }),
                    Err(_) => Picture::Failed,
                };
                this.pictures.insert(path, picture);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
