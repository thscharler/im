// #![windows_subsystem = "windows"]

use crate::image::Images;
use anyhow::Error;
use crossterm::event::Event;
use log::debug;
use rat_salsa_wgpu::events::{CompositeWinitEvent, ConvertCrosstermEx};
use rat_salsa_wgpu::image::ImageFit;
use rat_salsa_wgpu::poll::{PollBlink, PollTimers};
use rat_salsa_wgpu::timer::{TimeOut, TimerDef};
use rat_salsa_wgpu::{Control, RunConfig, SalsaAppContext, SalsaContext, run_tui};
use rat_theme4::theme::SalsaTheme;
use rat_theme4::{StyleName, WidgetStyle, create_salsa_theme};
use rat_widget::event::{Dialog, HandleEvent, Regular, ct_event, event_flow};
use rat_widget::focus::{FocusBuilder, FocusFlag, HasFocus, Navigation};
use rat_widget::msgdialog::MsgDialogState;
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

#[derive(Debug, Default)]
pub struct Scenery {
    pub tools: ToolbarState,
    pub text_overlay: TextAreaState,
    pub images: Images,
    pub error_dlg: MsgDialogState,
}

impl HasFocus for Scenery {
    fn build(&self, builder: &mut FocusBuilder) {
        builder.widget(&self.images);
        builder.widget_with_flags(
            self.text_overlay.focus.clone(),
            Rect::default(),
            0,
            Navigation::Regular,
        );
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
        .button("Up", " \u{21E7} ", false)
        .button("Dn", " \u{21E9} ", false)
        .text("  ")
        .button("F4", " -- ", false)
        .button("F5", " \u{25B6} ", false)
        .button("F6", " ++ ", false)
        .text("  ")
        .button("Del", " \u{2716} ", false)
        .button("", " \u{2716}\u{2716} ", false)
        .text("  ")
        .button("0", " \u{2921}\u{2922} ", false)
        .button("1..3", " \u{2194} ", false)
        .button("4..6", " \u{2194}\u{2195} ", false)
        .button("7..9", " \u{2195} ", false)
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
        ctx.set_focus(FocusBuilder::rebuild_for(state, ctx.take_focus()));
        ctx.focus().enable_log();
        ctx.handle_focus(event);

        match event {
            ct_event!(resized) => event_flow!(Control::Changed),
            ct_event!(key press CONTROL-'q') => event_flow!(Control::Quit),
            _ => {}
        }

        if state.text_overlay.is_focused() {
            event_flow!(state.text_overlay.handle(event, Regular));
        }
        let r = state.tools.handle(
            event,
            ToolbarKeys {
                focus: &*ctx.focus(),
                keys: [],
            },
        );
        match r {
            ToolbarOutcome::Pressed(0) => event_flow!({ image::prev_img(&mut state.images)? }),
            ToolbarOutcome::Pressed(1) => event_flow!({ image::next_img(&mut state.images)? }),
            ToolbarOutcome::Pressed(2) => {
                event_flow!({ image::decr_duration(&mut state.images, ctx)? })
            }
            ToolbarOutcome::Pressed(3) => {
                event_flow!({ image::play_pause(&mut state.images, ctx)? })
            }
            ToolbarOutcome::Pressed(4) => {
                event_flow!({ image::incr_duration(&mut state.images, ctx)? })
            }
            ToolbarOutcome::Pressed(5) => event_flow!({ image::del_img(&mut state.images)? }),
            ToolbarOutcome::Pressed(6) => {
                event_flow!({ image::clear_img(&mut state.images, ctx)? })
            }
            ToolbarOutcome::Pressed(7) => {
                event_flow!({ image::set_image_fit(&mut state.images, ImageFit::Fill)? })
            }
            ToolbarOutcome::Pressed(8) => event_flow!({
                let f = match image::image_fit(&mut state.images) {
                    None => ImageFit::HorizontalStart,
                    Some(ImageFit::HorizontalStart) => ImageFit::HorizontalCenter,
                    Some(ImageFit::HorizontalCenter) => ImageFit::HorizontalEnd,
                    Some(ImageFit::HorizontalEnd) => ImageFit::HorizontalStart,
                    Some(_) => ImageFit::HorizontalStart,
                };
                image::set_image_fit(&mut state.images, f)?
            }),
            ToolbarOutcome::Pressed(9) => event_flow!({
                let f = match image::image_fit(&mut state.images) {
                    None => ImageFit::FitStart,
                    Some(ImageFit::FitStart) => ImageFit::FitCenter,
                    Some(ImageFit::FitCenter) => ImageFit::FitEnd,
                    Some(ImageFit::FitEnd) => ImageFit::FitStart,
                    Some(_) => ImageFit::FitStart,
                };
                image::set_image_fit(&mut state.images, f)?
            }),
            ToolbarOutcome::Pressed(10) => event_flow!({
                let f = match image::image_fit(&mut state.images) {
                    None => ImageFit::VerticalStart,
                    Some(ImageFit::VerticalStart) => ImageFit::VerticalCenter,
                    Some(ImageFit::VerticalCenter) => ImageFit::VerticalEnd,
                    Some(ImageFit::VerticalEnd) => ImageFit::VerticalStart,
                    Some(_) => ImageFit::VerticalStart,
                };
                image::set_image_fit(&mut state.images, f)?
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
                debug!("dropped {:?}", f);
                event_flow!(Control::Event(ImEvent::AddImage(f.clone())))
            }
            WindowEvent::HoveredFile(_f) => event_flow!({ Control::Continue }),
            WindowEvent::HoveredFileCancelled => event_flow!({ Control::Continue }),
            _ => {}
        }
    }

    event_flow!(image::event(event, &mut state.images, ctx)?);

    Ok(Control::Continue)
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
    use std::time::Duration;

    #[derive(Debug, Default)]
    pub struct Images {
        area: Rect,

        idx: Option<usize>,
        images: Vec<ImageHandle>,
        fit: Vec<ImageFit>,
        timer: Option<TimerHandle>,
        timer_duration: Duration,

        focus: FocusFlag,
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
                ct_event!(key press '0') => event_flow!(set_image_fit(state, Fill)?),
                ct_event!(key press '1') => event_flow!(set_image_fit(state, HorizontalStart)?),
                ct_event!(key press '2') => event_flow!(set_image_fit(state, HorizontalCenter)?),
                ct_event!(key press '3') => event_flow!(set_image_fit(state, HorizontalEnd)?),
                ct_event!(key press '4') => event_flow!(set_image_fit(state, FitStart)?),
                ct_event!(key press '5') => event_flow!(set_image_fit(state, FitCenter)?),
                ct_event!(key press '6') => event_flow!(set_image_fit(state, FitEnd)?),
                ct_event!(key press '7') => event_flow!(set_image_fit(state, VerticalStart)?),
                ct_event!(key press '8') => event_flow!(set_image_fit(state, VerticalCenter)?),
                ct_event!(key press '9') => event_flow!(set_image_fit(state, VerticalEnd)?),

                ct_event!(keycode press Delete) => event_flow!(del_img(state)?),

                ct_event!(keycode press Up)
                | ct_event!(keycode press Left)
                | ct_event!(key press '+') => event_flow!(next_img(state,)?),

                ct_event!(keycode press Down)
                | ct_event!(keycode press Right)
                | ct_event!(key press '-') => event_flow!(prev_img(state,)?),

                ct_event!(scroll down) => event_flow!(next_img(state,)?),
                ct_event!(scroll up) => event_flow!(prev_img(state,)?),
                ct_event!(keycode press Media(media)) => match media {
                    MediaKeyCode::Play => event_flow!(play(state, ctx)?),
                    MediaKeyCode::Pause => event_flow!(pause(state, ctx)?),
                    MediaKeyCode::PlayPause => event_flow!(play_pause(state, ctx)?),
                    MediaKeyCode::Stop => event_flow!(pause(state, ctx)?),
                    MediaKeyCode::LowerVolume => event_flow!(decr_duration(state, ctx)?),
                    MediaKeyCode::RaiseVolume => event_flow!(incr_duration(state, ctx)?),
                    _ => {}
                },
                ct_event!(keycode press F(4)) | ct_event!(key press ALT-'-') => {
                    event_flow!(decr_duration(state, ctx)?)
                }
                ct_event!(keycode press F(5)) | ct_event!(key press '*') => {
                    event_flow!(play_pause(state, ctx)?)
                }
                ct_event!(keycode press F(6)) | ct_event!(key press ALT-'+') => {
                    event_flow!(incr_duration(state, ctx)?)
                }

                // todo: free drag
                _ => {}
            }
        }

        if let ImEvent::AddImage(im) = event {
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

            match state.idx {
                None => {
                    state.images.push(h_img);
                    state.fit.push(ImageFit::FitCenter);
                    state.idx = Some(0);
                }
                Some(idx) if idx + 1 < state.images.len() => {
                    let fit = state.fit[idx];
                    state.images.insert(idx + 1, h_img);
                    state.fit.insert(idx + 1, fit);
                    state.idx = Some(idx + 1);
                }
                Some(idx) => {
                    let fit = state.fit[idx];
                    state.images.push(h_img);
                    state.fit.push(fit);
                    state.idx = Some(state.images.len() - 1);
                }
            }

            event_flow!(Control::Changed);
        }

        if let ImEvent::TimeOut(t) = event {
            if state.timer == Some(t.handle) {
                event_flow!(next_img(state,)?);
            }
        }

        Ok(Control::Continue)
    }

    pub fn image_fit(state: &mut Images) -> Option<ImageFit> {
        if let Some(idx) = state.idx {
            Some(state.fit[idx])
        } else {
            None
        }
    }

    pub fn set_image_fit(state: &mut Images, fit: ImageFit) -> Result<Control<ImEvent>, Error> {
        if let Some(idx) = state.idx {
            state.fit[idx] = fit;
            Ok(Control::Changed)
        } else {
            Ok(Control::Continue)
        }
    }

    pub fn clear_img(state: &mut Images, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
        state.images.clear();
        state.fit.clear();
        state.idx = None;
        if let Some(timer) = state.timer.take() {
            ctx.remove_timer(timer);
        }
        Ok(Control::Changed)
    }

    pub fn del_img(state: &mut Images) -> Result<Control<ImEvent>, Error> {
        if let Some(idx) = state.idx {
            state.fit.remove(idx);
            state.images.remove(idx);
            if state.images.len() == 0 {
                state.idx = None;
            } else if idx >= state.images.len() {
                state.idx = Some(state.images.len() - 1);
            }
            Ok(Control::Changed)
        } else {
            Ok(Control::Continue)
        }
    }

    pub fn prev_img(state: &mut Images) -> Result<Control<ImEvent>, Error> {
        match &mut state.idx {
            None => Ok(Control::Continue),
            Some(idx) => {
                if *idx > 0 {
                    *idx -= 1;
                } else {
                    *idx = state.images.len() - 1;
                }
                Ok(Control::Changed)
            }
        }
    }

    pub fn next_img(state: &mut Images) -> Result<Control<ImEvent>, Error> {
        match &mut state.idx {
            None => Ok(Control::Continue),
            Some(idx) => {
                *idx += 1;
                if *idx >= state.images.len() {
                    *idx = 0;
                }
                Ok(Control::Changed)
            }
        }
    }

    pub fn incr_duration(
        state: &mut Images,
        ctx: &mut GlobalState,
    ) -> Result<Control<ImEvent>, Error> {
        if state.timer_duration.as_millis() > 1000 {
            state.timer_duration = state.timer_duration + Duration::from_millis(1000);
        } else {
            state.timer_duration = state.timer_duration + Duration::from_millis(100);
        }

        if state.timer.is_some() {
            state.timer = Some(ctx.replace_timer(
                state.timer,
                TimerDef::new().timer(state.timer_duration).repeat_forever(),
            ));
        }

        Ok(Control::Continue)
    }

    pub fn decr_duration(
        state: &mut Images,
        ctx: &mut GlobalState,
    ) -> Result<Control<ImEvent>, Error> {
        if state.timer_duration.as_millis() > 1000 {
            state.timer_duration = state.timer_duration - Duration::from_millis(1000);
        } else if state.timer_duration.as_millis() > 100 {
            state.timer_duration = state.timer_duration - Duration::from_millis(100);
        } else {
            // noop
        }

        if state.timer.is_some() {
            state.timer = Some(ctx.replace_timer(
                state.timer,
                TimerDef::new().timer(state.timer_duration).repeat_forever(),
            ));
        }

        Ok(Control::Changed)
    }

    pub fn play_pause(
        state: &mut Images,
        ctx: &mut GlobalState,
    ) -> Result<Control<ImEvent>, Error> {
        if state.timer.is_some() {
            pause(state, ctx)
        } else {
            play(state, ctx)
        }
    }

    pub fn play(state: &mut Images, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
        state.timer = Some(ctx.replace_timer(
            state.timer,
            TimerDef::new().timer(state.timer_duration).repeat_forever(),
        ));
        Ok(Control::Changed)
    }

    pub fn pause(state: &mut Images, ctx: &mut GlobalState) -> Result<Control<ImEvent>, Error> {
        if let Some(timer) = state.timer {
            ctx.remove_timer(timer);
        }
        Ok(Control::Changed)
    }
}

pub fn error(
    event: Error,
    state: &mut Scenery,
    _ctx: &mut GlobalState,
) -> Result<Control<ImEvent>, Error> {
    debug!("ERROR {:#?}", event);
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
