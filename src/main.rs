// #![windows_subsystem = "windows"]

use crate::image::Images;
use anyhow::Error;
use crossterm::event::Event;
use log::debug;
use rat_salsa_wgpu::events::{CompositeWinitEvent, ConvertCrosstermEx};
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
use rat_widget::toolbar::ToolbarState;
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
    pub textoverlay: TextAreaState,
    pub images: Images,
    pub error_dlg: MsgDialogState,
    pub toolbar: ToolbarState,
}

impl HasFocus for Scenery {
    fn build(&self, builder: &mut FocusBuilder) {
        builder.widget(&self.images);
        builder.widget_navigate(&self.textoverlay, Navigation::Regular);
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

    image::render(l0[1], buf, &mut state.images, ctx)?;

    let mut txt_style: TextStyle = ctx.theme.style(WidgetStyle::TEXTVIEW);
    txt_style.style.bg = None;
    let txt_area = l0[1].inner(Margin::new(1, 1));
    TextArea::new()
        .styles(txt_style)
        .render(txt_area, buf, &mut state.textoverlay);

    ctx.set_screen_cursor(state.textoverlay.screen_cursor());

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

        if state.textoverlay.is_focused() {
            event_flow!(state.textoverlay.handle(event, Regular));
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

        let px_area = img_buf.area_px();
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
                ct_event!(key press '7') => event_flow!(set_image_fit(state, FitVerticalStart)?),
                ct_event!(key press '8') => event_flow!(set_image_fit(state, FitVerticalCenter)?),
                ct_event!(key press '9') => event_flow!(set_image_fit(state, FitVerticalEnd)?),

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

    fn set_image_fit(state: &mut Images, fit: ImageFit) -> Result<Control<ImEvent>, Error> {
        if let Some(idx) = state.idx {
            state.fit[idx] = fit;
            Ok(Control::Changed)
        } else {
            Ok(Control::Continue)
        }
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
