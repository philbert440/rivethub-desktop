use std::sync::Arc;
use std::time::Duration;

use app::Hub;
use gpui::{
    AnyElement, Bounds, ClickEvent, Context, Entity, IntoElement, ParentElement, Pixels, Render, Styled,
    Subscription, Task, Window, canvas, div, fill, point, prelude::*, px, rgb, size,
};
use model::pairing::{PairingAvailability, PairingCode, PairingState, valid_phone_name};

use super::parts::{button, dim, labelled, primary, row};
use crate::input::{Submitted, TextInput};
use crate::text::now_ms;
use crate::theme::Theme;

/// How often a shown code is checked for the phone having used it.
const POLL: Duration = Duration::from_secs(2);
/// The widest the code is drawn; each module is a whole number of pixels within it.
const QR_SIZE: f32 = 280.;
/// The blank border a scanner needs around the code, in modules.
const QUIET: usize = 4;

enum Stage {
    Checking,
    Unavailable(String),
    /// Ready to make a code; `problem` is why the last try failed.
    Ready {
        problem: Option<String>,
    },
    Starting,
    Showing {
        code: PairingCode,
        qr: Option<Arc<Qr>>,
        state: PairingState,
        /// Why the last check on the code failed; the code still works until it lapses.
        unchecked: Option<String>,
    },
}

/// Pairs a phone with this node: the node makes a one-time code, shown as a QR code
/// the phone scans to get its own certificate.
pub struct PhonePairing {
    hub: Hub,
    stage: Stage,
    name: Entity<TextInput>,
    /// Watches a shown code for the phone using it.
    poll: Option<Task<()>>,
    _subscriptions: [Subscription; 2],
}

impl PhonePairing {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| TextInput::new("Phone name, such as pixel-8", cx));
        let typed = cx.observe(&name, |_, _, cx| cx.notify());
        let submitted = cx.subscribe(&name, |this, _, Submitted(_), cx| this.start(cx));
        Self {
            hub,
            stage: Stage::Checking,
            name,
            poll: None,
            _subscriptions: [typed, submitted],
        }
    }

    /// Asks the node again whether it can pair, unless a code is on screen.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if matches!(self.stage, Stage::Showing { .. } | Stage::Starting) {
            return;
        }
        self.stage = Stage::Checking;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let available = hub.phone_pairing().await;
            this.update(cx, |this, cx| {
                this.stage = match available {
                    Ok(PairingAvailability::Available { .. }) => Stage::Ready { problem: None },
                    Ok(PairingAvailability::Unavailable { reason }) => Stage::Unavailable(sentence(&reason)),
                    Err(err) => Stage::Unavailable(sentence(&err.to_string())),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.stage, Stage::Ready { .. }) {
            return;
        }
        let name = self.name.read(cx).text().trim().to_owned();
        if !valid_phone_name(&name) {
            return;
        }
        self.stage = Stage::Starting;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let started = hub.start_pairing(name).await;
            this.update(cx, |this, cx| {
                match started {
                    Ok(code) => this.show(code, cx),
                    Err(err) => {
                        this.stage = Stage::Ready {
                            problem: Some(err.to_string()),
                        };
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn show(&mut self, code: PairingCode, cx: &mut Context<Self>) {
        self.name.update(cx, |name, cx| name.set_text("", cx));
        let qr = Qr::encode(&code.qr_text).map(Arc::new);
        let device = code.device_id.clone();
        let expires = code.expires_at_ms;
        self.stage = Stage::Showing {
            code,
            qr,
            state: PairingState::Pending,
            unchecked: None,
        };
        let hub = self.hub.clone();
        self.poll = Some(cx.spawn(async move |this, cx| {
            while now_ms() < expires {
                cx.background_executor().timer(POLL).await;
                let state = hub.pairing_state(device.clone()).await;
                let pending = this
                    .update(cx, |this, cx| {
                        let Stage::Showing {
                            state: shown,
                            unchecked,
                            ..
                        } = &mut this.stage
                        else {
                            return false;
                        };
                        match state {
                            Ok(state) => {
                                *shown = state;
                                *unchecked = None;
                            }
                            Err(err) => *unchecked = Some(err.to_string()),
                        }
                        cx.notify();
                        *shown == PairingState::Pending
                    })
                    .unwrap_or(false);
                if !pending {
                    return;
                }
            }
            this.update(cx, |_, cx| cx.notify()).ok();
        }));
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.poll = None;
        self.stage = Stage::Ready { problem: None };
        cx.notify();
    }

    fn form(&self, problem: Option<&str>, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let typed = self.name.read(cx).text().trim().to_owned();
        let valid = valid_phone_name(&typed);
        let hint = match (problem, typed.is_empty(), valid) {
            (Some(problem), _, _) => Some((problem.to_owned(), Theme::warn())),
            (None, false, false) => Some((
                "Use letters, digits, “.”, “_” and “-” only.".to_owned(),
                Theme::warn(),
            )),
            _ => None,
        };
        div()
            .px_5()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(dim("Connect RivetHub on Android by scanning a code. The phone gets its own certificate, and nothing is typed on it.").text_xs())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .rounded(px(10.))
                            .bg(Theme::surface())
                            .border_1()
                            .border_color(Theme::line())
                            .child(self.name.clone()),
                    )
                    .child(
                        primary("show-pairing-code", if busy { "Preparing…" } else { "Show pairing code" })
                            .when(!valid || busy, |button| button.opacity(0.5).cursor_default())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start(cx))),
                    ),
            )
            .children(hint.map(|(text, tone)| div().text_xs().text_color(tone).child(text)))
            .into_any_element()
    }

    fn code(
        code: &PairingCode,
        qr: Option<&Arc<Qr>>,
        state: PairingState,
        unchecked: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = now_ms();
        let state = if state == PairingState::Pending && code.lapsed(now) {
            PairingState::Expired
        } else {
            state
        };
        let close = button(
            "close-pairing",
            if state == PairingState::Pending {
                "Cancel"
            } else {
                "Close"
            },
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close(cx)));
        let body = match state {
            PairingState::Paired => div().py_6().text_color(Theme::accent()).child(format!(
                "Paired with {}. RivetHub on the phone is connecting.",
                code.name
            )),
            PairingState::Expired => div()
                .py_6()
                .text_color(Theme::ink_dim())
                .child("This code expired or was used. Show a new one."),
            PairingState::Pending => div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .children(qr.map(Qr::element))
                .when(qr.is_none(), |body| {
                    body.child(
                        div()
                            .text_color(Theme::warn())
                            .child("This code is too long to draw."),
                    )
                })
                .child("Scan it with the phone's camera, or open RivetHub and tap Scan pairing QR.")
                .child(
                    dim(format!(
                        "{} · reaches this computer at {} · works once · expires in {}m",
                        code.name,
                        code.gateway.trim_start_matches("https://"),
                        code.minutes_left(now)
                    ))
                    .text_xs(),
                )
                .children(unchecked.map(|problem| {
                    div()
                        .text_xs()
                        .text_color(Theme::warn())
                        .child(format!("Couldn't check whether the phone used it: {problem}"))
                })),
        };
        div()
            .px_5()
            .py_4()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(body)
            .child(close)
            .into_any_element()
    }
}

impl Render for PhonePairing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.stage {
            Stage::Checking => row().child(dim("Checking…")).into_any_element(),
            Stage::Unavailable(reason) => row()
                .child(labelled("Pairing isn't available here", reason.clone()))
                .into_any_element(),
            Stage::Ready { problem } => {
                let problem = problem.clone();
                self.form(problem.as_deref(), false, cx)
            }
            Stage::Starting => self.form(None, true, cx),
            Stage::Showing {
                code,
                qr,
                state,
                unchecked,
            } => {
                let (code, qr, state, unchecked) = (code.clone(), qr.clone(), *state, unchecked.clone());
                Self::code(&code, qr.as_ref(), state, unchecked.as_deref(), cx)
            }
        }
    }
}

/// rh's reasons start in lower case, as error messages do; a row reads them as a sentence.
fn sentence(reason: &str) -> String {
    let mut chars = reason.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect::<String>() + "."
    })
}

/// A QR code's dark modules, row by row.
struct Qr {
    width: usize,
    dark: Vec<bool>,
}

impl Qr {
    fn encode(text: &str) -> Option<Self> {
        let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).ok()?;
        Some(Self {
            width: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|color| color == qrcode::Color::Dark)
                .collect(),
        })
    }

    /// Dark modules on white whatever the theme, as scanners expect, with a quiet border.
    fn element(self: &Arc<Self>) -> AnyElement {
        let across = self.width + 2 * QUIET;
        #[expect(clippy::cast_precision_loss, reason = "a QR code is at most 177 modules wide")]
        let module = (QR_SIZE / across as f32).floor().max(1.);
        #[expect(clippy::cast_precision_loss, reason = "as above")]
        let side = module * across as f32;
        let qr = Arc::clone(self);
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, (), window, _| {
                window.paint_quad(fill(bounds, rgb(0x00ff_ffff)));
                let ink = rgb(0x000d_1117);
                for (index, _) in qr.dark.iter().enumerate().filter(|(_, dark)| **dark) {
                    let (y, x) = (index / qr.width + QUIET, index % qr.width + QUIET);
                    #[expect(clippy::cast_precision_loss, reason = "as above")]
                    let origin = point(
                        bounds.origin.x + px(module * x as f32),
                        bounds.origin.y + px(module * y as f32),
                    );
                    window.paint_quad(fill(Bounds::new(origin, size(px(module), px(module))), ink));
                }
            },
        )
        .size(px(side))
        .into_any_element()
    }
}
