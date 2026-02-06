// #![windows_subsystem = "windows"]

use crate::image::Images;
use anyhow::Error;
use crossterm::event::Event;
use log::error;
use rat_salsa_wgpu::events::{CompositeWinitEvent, ConvertCrosstermEx};
use rat_salsa_wgpu::image::ImageFit;
use rat_salsa_wgpu::poll::{PollBlink, PollTimers};
use rat_salsa_wgpu::timer::TimeOut;
use rat_salsa_wgpu::{Control, RunConfig, SalsaAppContext, SalsaContext, run_tui};
use rat_theme4::theme::SalsaTheme;
use rat_theme4::{StyleName, WidgetStyle, create_salsa_theme};
use rat_widget::event::{Dialog, HandleEvent, Regular, TextOutcome, ct_event, event_flow};
use rat_widget::focus::{FocusBuilder, FocusFlag, HasFocus, Navigation};
use rat_widget::msgdialog::MsgDialogState;
use rat_widget::text::clipboard::cli::setup_cli_clipboard;
use rat_widget::text::{HasScreenCursor, TextStyle};
use rat_widget::textarea::{TextArea, TextAreaState};
use rat_widget::toolbar::{Toolbar, ToolbarKeys, ToolbarOutcome, ToolbarState};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::widgets::StatefulWidget;
use std::fs;
use std::path::PathBuf;
use winit::event::WindowEvent;

fn main() -> Result<(), Error> {
    setup_logging()?;
    setup_cli_clipboard();

    let run_config = RunConfig::new(ConvertCrosstermEx::new())?
        .poll(PollBlink::default())
        .poll(PollTimers::new())
        .backends("*")
        .window_title("im");

    let theme = create_salsa_theme("EverForest Light");
    let mut global = GlobalState::new(theme);
    let mut state = Scenery::default();

    run_tui(
        init,
        render,
        event,
        error,
        &mut global,
        &mut state,
        run_config,
    )?;

    Ok(())
}

pub struct GlobalState {
    pub ctx: SalsaAppContext<ImEvent, Error>,
    pub theme: SalsaTheme,
}

impl SalsaContext<ImEvent, Error> for GlobalState {
    fn set_salsa_ctx(&mut self, app_ctx: SalsaAppContext<ImEvent, Error>) {
        self.ctx = app_ctx;
    }

    #[inline]
    fn salsa_ctx(&self) -> &SalsaAppContext<ImEvent, Error> {
        &self.ctx
    }
}

impl GlobalState {
    pub fn new(theme: SalsaTheme) -> Self {
        Self {
            ctx: Default::default(),
            theme,
        }
    }
}

#[derive(Debug)]
pub enum ImEvent {
    Noop,
    Event(Event),
    Winit(CompositeWinitEvent),
    AddImage(PathBuf),
    TimeOut(TimeOut),
}

impl From<TimeOut> for ImEvent {
    fn from(value: TimeOut) -> Self {
        ImEvent::TimeOut(value)
    }
}

impl From<Event> for ImEvent {
    fn from(value: Event) -> Self {
        ImEvent::Event(value)
    }
}

impl From<CompositeWinitEvent> for ImEvent {
    fn from(value: CompositeWinitEvent) -> Self {
        ImEvent::Winit(value)
    }
}

#[derive(Debug)]
pub struct Scenery {
    pub tools: ToolbarState,
    pub text_overlay: TextAreaState,
    pub images: Images,
    pub error_dlg: MsgDialogState,
}

impl Default for Scenery {
    fn default() -> Self {
        Self {
            tools: Default::default(),
            text_overlay: TextAreaState::named("text"),
            images: Images::named("images"),
            error_dlg: Default::default(),
        }
    }
}

impl HasFocus for Scenery {
    fn build(&self, builder: &mut FocusBuilder) {
        builder.widget_navigate(&self.text_overlay, Navigation::Regular);
        builder.widget(&self.images);
    }

    fn focus(&self) -> FocusFlag {
        unimplemented!()
    }

    fn area(&self) -> Rect {
        unimplemented!()
    }
}

pub fn init(state: &mut Scenery, ctx: &mut GlobalState) -> Result<(), Error> {
    ctx.set_focus(FocusBuilder::build_for(state));
    image::init(&mut state.images, ctx)?;
    Ok(())
}

pub fn render(
    area: Rect,
    buf: &mut Buffer,
    state: &mut Scenery,
    ctx: &mut GlobalState,
) -> Result<(), Error> {
    let l0 = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).split(area);

    let style = ctx.theme.style_style(Style::CONTAINER_BASE);
    ctx.set_bg_color(style.bg.unwrap_or_default());
    ctx.set_fg_color(style.fg.unwrap_or_default());
    buf.set_style(area, style);

    let (tool, tool_popup) = Toolbar::new()
        .styles(ctx.theme.style(WidgetStyle::TOOLBAR))
        .button("", " \u{21E7} ", false)
        .button("", " \u{21E9} ", false)
        .button("", " T ", false)
        .text("  ")
        .button("", " -- ", false)
        .button("", " \u{25B6} ", false)
        .button("", " ++ ", false)
        .text("  ")
        .button("", " \u{2716} ", false)
        .button("", " \u{2716}\u{2716} ", false)
        .text("  ")
        .button("", " \u{2921}\u{2922} ", false)
        .button("", " \u{2194} ", false)
        .button("", " \u{2194}\u{2195} ", false)
        .button("", " \u{2195} ", false)
        .into_widgets(l0[0], &mut state.tools);
    tool.render(l0[0], buf, &mut state.tools);

    image::render(l0[1], buf, &mut state.images, ctx)?;

    let mut txt_style: TextStyle = ctx.theme.style(WidgetStyle::TEXTVIEW);
    txt_style.style.bg = None;
    let txt_area = l0[1].inner(Margin::new(1, 1));
    TextArea::new()
        .styles(txt_style)
        .render(txt_area, buf, &mut state.text_overlay);

    tool_popup.render(l0[0], buf, &mut state.tools);

    ctx.set_screen_cursor(state.text_overlay.screen_cursor());

    Ok(())
}

pub fn event(
    event: &ImEvent,
    state: &mut Scenery,
    ctx: &mut GlobalState,
) -> Result<Control<ImEvent>, Error> {
    if let ImEvent::Event(event) = event {
        match event {
            ct_event!(resized) => event_flow!(Control::Changed),
            ct_event!(key press CONTROL-'q') => event_flow!(Control::Quit),
            ct_event!(keycode press F(1)) => event_flow!(flip_text(state, ctx)?),
            _ => {}
        }

        if state.text_overlay.is_focused() {
            event_flow!(match state.text_overlay.handle(event, Regular) {
                TextOutcome::TextChanged => {
                    let txt = state.text_overlay.value();
                    state.images.set_text(txt.clone());
                    if !txt.is_empty() {
                        if let Some(txt_file) = state.images.text_file() {
                            _ = fs::write(txt_file, txt);
                        }
                    }
                    TextOutcome::TextChanged
                }
                r => r,
            });
        }

        let r = state.tools.handle(
            event,
            ToolbarKeys {
                focus: &*ctx.focus(),
                keys: [],
            },
        );
        match r {
            ToolbarOutcome::Pressed(0) => event_flow!({ state.images.prev_img()? }),
            ToolbarOutcome::Pressed(1) => event_flow!({ state.images.next_img()? }),
            ToolbarOutcome::Pressed(2) => event_flow!({ flip_text(state, ctx)? }),
            ToolbarOutcome::Pressed(3) => {
                event_flow!({ state.images.decr_duration(ctx)? })
            }
            ToolbarOutcome::Pressed(4) => {
                event_flow!({ state.images.play_pause(ctx)? })
            }
            ToolbarOutcome::Pressed(5) => {
                event_flow!({ state.images.incr_duration(ctx)? })
            }
            ToolbarOutcome::Pressed(6) => event_flow!({ state.images.del_img()? }),
            ToolbarOutcome::Pressed(7) => {
                event_flow!({ state.images.clear_img(ctx)? })
            }
            ToolbarOutcome::Pressed(8) => {
                event_flow!({ state.images.set_image_fit(ImageFit::Fill)? })
            }
            ToolbarOutcome::Pressed(9) => event_flow!({
                let f = match state.images.image_fit() {
                    None => ImageFit::HorizontalStart,
                    Some(ImageFit::HorizontalStart) => ImageFit::HorizontalCenter,
                    Some(ImageFit::HorizontalCenter) => ImageFit::HorizontalEnd,
                    Some(ImageFit::HorizontalEnd) => ImageFit::HorizontalStart,
                    Some(_) => ImageFit::HorizontalStart,
                };
                state.images.set_image_fit(f)?
            }),
            ToolbarOutcome::Pressed(10) => event_flow!({
                let f = match state.images.image_fit() {
                    None => ImageFit::FitStart,
                    Some(ImageFit::FitStart) => ImageFit::FitCenter,
                    Some(ImageFit::FitCenter) => ImageFit::FitEnd,
                    Some(ImageFit::FitEnd) => ImageFit::FitStart,
                    Some(_) => ImageFit::FitStart,
                };
                state.images.set_image_fit(f)?
            }),
            ToolbarOutcome::Pressed(11) => event_flow!({
                let f = match state.images.image_fit() {
                    None => ImageFit::VerticalStart,
                    Some(ImageFit::VerticalStart) => ImageFit::VerticalCenter,
                    Some(ImageFit::VerticalCenter) => ImageFit::VerticalEnd,
                    Some(ImageFit::VerticalEnd) => ImageFit::VerticalStart,
                    Some(_) => ImageFit::VerticalStart,
                };
                state.images.set_image_fit(f)?
            }),
            r => event_flow!(r),
        }

        if state.error_dlg.active() {
            event_flow!(state.error_dlg.handle(event, Dialog));
        }
    }

    if let ImEvent::Winit(winit) = event {
        match &winit.event {
            WindowEvent::DroppedFile(f) => {
                event_flow!(Control::Event(ImEvent::AddImage(f.clone())))
            }
            WindowEvent::HoveredFile(_f) => event_flow!({ Control::Continue }),
            WindowEvent::HoveredFileCancelled => event_flow!({ Control::Continue }),
            _ => {}
        }
    }

    event_flow!(match image::event(event, &mut state.images, ctx)? {
        Control::Changed => {
            state.text_overlay.set_text(state.images.text());
            Control::Changed
        }
        r => r,
    });

    Ok(Control::Continue)
}

fn flip_text(state: &mut Scenery, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
    if !state.text_overlay.is_focused() {
        ctx.focus().focus(&state.text_overlay);
    } else {
        ctx.focus().focus(&state.images);
    }
    Ok(Control::Changed)
}

mod image {
    use crate::{GlobalState, ImEvent};
    use anyhow::Error;
    use crossterm::event::MediaKeyCode;
    use image::ImageReader;
    use rat_salsa_wgpu::image::{ImageArg, ImageFit, ImageHandle};
    use rat_salsa_wgpu::timer::{TimerDef, TimerHandle};
    use rat_salsa_wgpu::{Control, SalsaContext};
    use rat_widget::event::{ct_event, event_flow};
    use rat_widget::focus::{FocusBuilder, FocusFlag, HasFocus};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::fs::read_to_string;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[derive(Debug, Default)]
    pub struct Images {
        area: Rect,

        idx: Option<usize>,
        images: Vec<ImageHandle>,
        text_file: Vec<PathBuf>,
        text: Vec<String>,
        fit: Vec<ImageFit>,
        timer: Option<TimerHandle>,
        timer_duration: Duration,

        focus: FocusFlag,
    }

    impl Images {
        pub fn named(name: &str) -> Self {
            Self {
                area: Default::default(),
                idx: Default::default(),
                images: Default::default(),
                text_file: Default::default(),
                text: Default::default(),
                fit: Default::default(),
                timer: Default::default(),
                timer_duration: Default::default(),
                focus: FocusFlag::new().with_name(name),
            }
        }
    }

    impl HasFocus for Images {
        fn build(&self, builder: &mut FocusBuilder) {
            builder.leaf_widget(self);
        }

        fn focus(&self) -> FocusFlag {
            self.focus.clone()
        }

        fn area(&self) -> Rect {
            self.area
        }
    }

    pub fn init(state: &mut Images, _ctx: &mut GlobalState) -> Result<(), Error> {
        state.timer_duration = Duration::from_millis(1000);
        Ok(())
    }

    pub fn render(
        area: Rect,
        _buf: &mut Buffer,
        state: &mut Images,
        ctx: &mut GlobalState,
    ) -> Result<(), Error> {
        let Some(idx) = state.idx else { return Ok(()) };

        state.area = area;

        let img_buf = ctx.image_buffer();
        let mut img_buf = img_buf.lock().expect("lock");

        let img_handle = &state.images[idx];
        let img_fit = state.fit[idx];

        let px_area = img_buf.rect_px(area);
        img_buf.render_px(
            img_handle,
            px_area,
            ImageArg::new().fit(img_fit).clip(px_area).below_text(),
        );

        Ok(())
    }

    pub fn event(
        event: &ImEvent,
        state: &mut Images,
        ctx: &mut GlobalState,
    ) -> Result<Control<ImEvent>, Error> {
        if let ImEvent::Event(event) = event
            && state.is_focused()
        {
            use ImageFit::*;
            match event {
                ct_event!(key press '0') => event_flow!(state.set_image_fit(Fill)?),
                ct_event!(key press '1') => event_flow!(state.set_image_fit(HorizontalStart)?),
                ct_event!(key press '2') => event_flow!(state.set_image_fit(HorizontalCenter)?),
                ct_event!(key press '3') => event_flow!(state.set_image_fit(HorizontalEnd)?),
                ct_event!(key press '4') => event_flow!(state.set_image_fit(FitStart)?),
                ct_event!(key press '5') => event_flow!(state.set_image_fit(FitCenter)?),
                ct_event!(key press '6') => event_flow!(state.set_image_fit(FitEnd)?),
                ct_event!(key press '7') => event_flow!(state.set_image_fit(VerticalStart)?),
                ct_event!(key press '8') => event_flow!(state.set_image_fit(VerticalCenter)?),
                ct_event!(key press '9') => event_flow!(state.set_image_fit(VerticalEnd)?),

                ct_event!(keycode press Delete) => event_flow!(state.del_img()?),

                ct_event!(keycode press Up)
                | ct_event!(keycode press Left)
                | ct_event!(key press '+') => event_flow!(state.next_img()?),

                ct_event!(keycode press Down)
                | ct_event!(keycode press Right)
                | ct_event!(key press '-') => event_flow!(state.prev_img()?),

                ct_event!(scroll down) => event_flow!(state.next_img()?),
                ct_event!(scroll up) => event_flow!(state.prev_img()?),
                ct_event!(keycode press Media(media)) => match media {
                    MediaKeyCode::Play => event_flow!(state.play(ctx)?),
                    MediaKeyCode::Pause => event_flow!(state.pause(ctx)?),
                    MediaKeyCode::PlayPause => event_flow!(state.play_pause(ctx)?),
                    MediaKeyCode::Stop => event_flow!(state.pause(ctx)?),
                    MediaKeyCode::LowerVolume => event_flow!(state.decr_duration(ctx)?),
                    MediaKeyCode::RaiseVolume => event_flow!(state.incr_duration(ctx)?),
                    _ => {}
                },
                ct_event!(keycode press F(4)) | ct_event!(key press ALT-'-') => {
                    event_flow!(state.decr_duration(ctx)?)
                }
                ct_event!(keycode press F(5)) | ct_event!(key press '*') => {
                    event_flow!(state.play_pause(ctx)?)
                }
                ct_event!(keycode press F(6)) | ct_event!(key press ALT-'+') => {
                    event_flow!(state.incr_duration(ctx)?)
                }

                // todo: free drag
                _ => {}
            }
        }

        if let ImEvent::AddImage(im) = event {
            event_flow!(state.add_img(im, ctx)?)
        }

        if let ImEvent::TimeOut(t) = event {
            if state.timer == Some(t.handle) {
                event_flow!(state.next_img()?);
            }
        }

        Ok(Control::Continue)
    }

    impl Images {
        pub fn text(&self) -> &str {
            if let Some(idx) = self.idx {
                &self.text[idx]
            } else {
                ""
            }
        }

        pub fn set_text(&mut self, txt: String) {
            if let Some(idx) = self.idx {
                self.text[idx] = txt;
            } else {
                // noop
            }
        }

        pub fn text_file(&self) -> Option<&Path> {
            if let Some(idx) = self.idx {
                Some(&self.text_file[idx])
            } else {
                None
            }
        }

        pub fn add_img(
            &mut self,
            im: &Path,
            ctx: &mut GlobalState,
        ) -> Result<Control<ImEvent>, Error> {
            let txt_path = im.with_extension(".txt");
            let txt_str = read_to_string(&txt_path).unwrap_or_default();

            let image = ImageReader::open(im)?;
            let image = image.decode()?;
            let rgba = image.to_rgba8();
            let rgba = rgba.into_flat_samples();

            let (_c, w, h) = rgba.extents();

            let h_img = ctx.terminal().borrow_mut().backend_mut().add_image(
                &rgba.samples,
                w as u32,
                h as u32,
            );

            match self.idx {
                None => {
                    self.images.push(h_img);
                    self.text.push(txt_str);
                    self.text_file.push(txt_path);
                    self.fit.push(ImageFit::FitCenter);
                    self.idx = Some(0);
                }
                Some(idx) if idx + 1 < self.images.len() => {
                    let fit = self.fit[idx];
                    self.images.insert(idx + 1, h_img);
                    self.text.insert(idx + 1, txt_str);
                    self.text_file.insert(idx + 1, txt_path);
                    self.fit.insert(idx + 1, fit);
                    self.idx = Some(idx + 1);
                }
                Some(idx) => {
                    let fit = self.fit[idx];
                    self.images.push(h_img);
                    self.text.push(txt_str);
                    self.text_file.push(txt_path);
                    self.fit.push(fit);
                    self.idx = Some(self.images.len() - 1);
                }
            }

            Ok(Control::Changed)
        }

        pub fn clear_img(&mut self, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
            self.images.clear();
            self.text.clear();
            self.text_file.clear();
            self.fit.clear();
            self.idx = None;
            if let Some(timer) = self.timer.take() {
                ctx.remove_timer(timer);
            }
            Ok(Control::Changed)
        }

        pub fn del_img(&mut self) -> Result<Control<ImEvent>, Error> {
            if let Some(idx) = self.idx {
                self.fit.remove(idx);
                self.text.remove(idx);
                self.text_file.remove(idx);
                self.images.remove(idx);
                if self.images.len() == 0 {
                    self.idx = None;
                } else if idx >= self.images.len() {
                    self.idx = Some(self.images.len() - 1);
                }
                Ok(Control::Changed)
            } else {
                Ok(Control::Continue)
            }
        }

        pub fn image_fit(&mut self) -> Option<ImageFit> {
            if let Some(idx) = self.idx {
                Some(self.fit[idx])
            } else {
                None
            }
        }

        pub fn set_image_fit(&mut self, fit: ImageFit) -> Result<Control<ImEvent>, Error> {
            if let Some(idx) = self.idx {
                self.fit[idx] = fit;
                Ok(Control::Changed)
            } else {
                Ok(Control::Continue)
            }
        }

        pub fn prev_img(&mut self) -> Result<Control<ImEvent>, Error> {
            match &mut self.idx {
                None => Ok(Control::Continue),
                Some(idx) => {
                    if *idx > 0 {
                        *idx -= 1;
                    } else {
                        *idx = self.images.len() - 1;
                    }
                    Ok(Control::Changed)
                }
            }
        }

        pub fn next_img(&mut self) -> Result<Control<ImEvent>, Error> {
            match &mut self.idx {
                None => Ok(Control::Continue),
                Some(idx) => {
                    *idx += 1;
                    if *idx >= self.images.len() {
                        *idx = 0;
                    }
                    Ok(Control::Changed)
                }
            }
        }

        pub fn incr_duration(&mut self, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
            if self.timer_duration.as_millis() > 1000 {
                self.timer_duration = self.timer_duration + Duration::from_millis(500);
            } else {
                self.timer_duration = self.timer_duration + Duration::from_millis(100);
            }

            if self.timer.is_some() {
                self.timer = Some(ctx.replace_timer(
                    self.timer,
                    TimerDef::new().timer(self.timer_duration).repeat_forever(),
                ));
            }

            Ok(Control::Continue)
        }

        pub fn decr_duration(&mut self, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
            if self.timer_duration.as_millis() > 1000 {
                self.timer_duration = self.timer_duration - Duration::from_millis(500);
            } else if self.timer_duration.as_millis() > 100 {
                self.timer_duration = self.timer_duration - Duration::from_millis(100);
            } else {
                // noop
            }

            if self.timer.is_some() {
                self.timer = Some(ctx.replace_timer(
                    self.timer,
                    TimerDef::new().timer(self.timer_duration).repeat_forever(),
                ));
            }

            Ok(Control::Changed)
        }

        pub fn play_pause(&mut self, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
            if self.timer.is_some() {
                self.pause(ctx)
            } else {
                self.play(ctx)
            }
        }

        pub fn play(&mut self, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
            self.timer = Some(ctx.replace_timer(
                self.timer,
                TimerDef::new().timer(self.timer_duration).repeat_forever(),
            ));
            Ok(Control::Changed)
        }

        pub fn pause(&mut self, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
            if let Some(timer) = self.timer {
                ctx.remove_timer(timer);
            }
            Ok(Control::Changed)
        }
    }
}

pub fn error(
    event: Error,
    state: &mut Scenery,
    _ctx: &mut GlobalState,
) -> Result<Control<ImEvent>, Error> {
    error!("ERROR {:#?}", event);
    state.error_dlg.append(format!("{:?}", &*event).as_str());
    Ok(Control::Changed)
}

fn setup_logging() -> Result<(), Error> {
    if let Some(_cache) = dirs::cache_dir() {
        // let log_path = cache.join("rat-salsa");
        let log_path = PathBuf::from(".");
        if !log_path.exists() {
            fs::create_dir_all(&log_path)?;
        }

        let log_file = log_path.join("im.log");
        _ = fs::remove_file(&log_file);
        fern::Dispatch::new()
            .format(|out, message, _record| {
                out.finish(format_args!("{}", message)) //
            })
            .level(log::LevelFilter::Debug)
            .chain(fern::log_file(&log_file)?)
            .apply()?;
    }
    Ok(())
}
