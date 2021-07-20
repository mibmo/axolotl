use argh::FromArgs;
use async_std::{
    channel::{self, Receiver, Sender},
    task,
};
use console::Term;
use sailfish::TemplateOnce;
use winit::{
    event::{
        ButtonId, DeviceEvent, ElementState, Event, KeyboardInput, MouseScrollDelta, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

const DEFAULT_KEYBOARD_BUFFER_SIZE: usize = 12;
const DEFAULT_MOUSE_BUFFER_SIZE: usize = 12;

/// The cross-platform hardware device event lister
#[derive(FromArgs)]
struct Opt {
    /// the amount of keyboard events that can be displayed at once
    #[argh(option, short = 'k')]
    max_keyboard_events: Option<usize>,

    /// the amount of keyboard events that can be displayed at once
    #[argh(option, short = 'm')]
    max_mouse_events: Option<usize>,
}

type EResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone)]
pub(crate) enum HandledEvent {
    Keyboard(KeyboardInput),
    MouseButton {
        button: ButtonId,
        state: ElementState,
    },
    MouseScroll(MouseScrollDelta),
}

mod template {
    use crate::HandledEvent;
    use sailfish::TemplateOnce;

    #[derive(Debug, TemplateOnce)]
    #[template(path = "all.stpl")]
    pub(crate) struct All {
        // @TODO: unify into one vec and make it the templates' job to display the right events
        pub keyboard_events: Vec<HandledEvent>,
        pub mouse_events: Vec<HandledEvent>,
    }
}

#[async_std::main]
async fn main() -> EResult<()> {
    let opt: Opt = argh::from_env();

    let (event_tx, event_rx) = channel::unbounded();
    let tui = task::Builder::new()
        .name("Terminal User Interface".to_string())
        .spawn(tui_loop(opt, event_rx))?;
    window_loop(event_tx)?; // has to run on main thread due to cross-platform stuff

    tui.await
        .expect("an error occured in the terminal user interface");
    Ok(())
}

async fn tui_loop(opt: Opt, rx: Receiver<HandledEvent>) -> EResult<()> {
    let term = Term::stdout();

    let keyboard_buffer_size = opt
        .max_keyboard_events
        .unwrap_or(DEFAULT_KEYBOARD_BUFFER_SIZE);
    let mouse_buffer_size = opt.max_mouse_events.unwrap_or(DEFAULT_MOUSE_BUFFER_SIZE);
    let mut keyboard_event_buffer = Vec::with_capacity(keyboard_buffer_size);
    let mut mouse_event_buffer = Vec::with_capacity(mouse_buffer_size);

    while let Ok(event) = rx.recv().await {
        match event {
            HandledEvent::Keyboard(_) => &mut keyboard_event_buffer,
            HandledEvent::MouseButton { .. } | HandledEvent::MouseScroll(_) => {
                &mut mouse_event_buffer
            }
        }
        .push(event);

        // clear buffers
        if keyboard_event_buffer.len() > keyboard_buffer_size {
            keyboard_event_buffer.remove(0);
        }
        if mouse_event_buffer.len() > mouse_buffer_size {
            mouse_event_buffer.remove(0);
        }

        // render template in advance to avoid unnecessary waiting
        let template = template::All {
            keyboard_events: keyboard_event_buffer.clone(),
            mouse_events: mouse_event_buffer.clone(),
        };
        let render = template.render_once()?;

        // redraw
        term.clear_screen()?;
        term.write_str(&render)?;
    }

    Ok(())
}

fn window_loop(tx: Sender<HandledEvent>) -> EResult<()> {
    let event_loop = EventLoop::new();
    let window = Window::new(&event_loop)?;
    window.set_visible(false);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        let tx = tx.clone();
        match event {
            // keyboard input
            Event::DeviceEvent {
                event: DeviceEvent::Key(key),
                ..
            } => {
                task::spawn(async move { tx.send(HandledEvent::Keyboard(key)).await });
            }
            // mouse buttons
            Event::DeviceEvent {
                event: DeviceEvent::Button { button, state },
                ..
            } => {
                task::spawn(
                    async move { tx.send(HandledEvent::MouseButton { button, state }).await },
                );
            }
            // mouse scroll whell
            Event::DeviceEvent {
                event: DeviceEvent::MouseWheel { delta },
                ..
            } => {
                task::spawn(async move { tx.send(HandledEvent::MouseScroll(delta)).await });
            }
            // graceful exit
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
