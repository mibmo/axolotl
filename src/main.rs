use argh::FromArgs;
use async_std::{
    channel::{self, Receiver, Sender},
    task,
};
use console::Term;
use sailfish::TemplateOnce;
use std::collections::HashMap;
use winit::{
    event::{
        ButtonId, DeviceEvent, ElementState, Event, KeyboardInput, MouseScrollDelta, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

const DEFAULT_EVENT_BUFFER_SIZE: usize = 6;

/// The cross-platform hardware device event lister
#[derive(FromArgs)]
struct Opt {
    /// total amount of events that can be displayed at once
    #[argh(option, short = 'm')]
    max_events: Option<usize>,
}

type EResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const HANDLED_EVENT_VARIANTS: usize = 3; // not using syn; want to have acceptable compile-times
#[derive(Debug, Clone)]
pub(crate) enum HandledEvent {
    Keyboard(KeyboardInput),
    MouseButton {
        button: ButtonId,
        state: ElementState,
    },
    MouseScroll(MouseScrollDelta),
}

impl HandledEvent {
    fn variant(&self) -> &'static str {
        match self {
            HandledEvent::Keyboard(_) => "Keyboard",
            HandledEvent::MouseButton { .. } => "MouseButton",
            HandledEvent::MouseScroll(_) => "MouseScroll",
        }
    }
}

mod template {
    use crate::HandledEvent;
    use sailfish::TemplateOnce;
    use std::collections::HashMap;

    #[derive(Debug, TemplateOnce)]
    #[template(path = "all.stpl")]
    pub(crate) struct All {
        pub events: Vec<HandledEvent>,
        pub buffer_size: usize,
        pub event_totals: HashMap<&'static str, usize>,
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

    let buffer_size = opt.max_events.unwrap_or(DEFAULT_EVENT_BUFFER_SIZE);
    let mut event_buffer = Vec::with_capacity(buffer_size * HANDLED_EVENT_VARIANTS); // accomedate for combined variants size
    let mut variant_count: HashMap<&'static str, usize> = HashMap::new();

    // initialise hashmap with count defaults
    variant_count.insert("Keyboard", 0);
    variant_count.insert("MouseButton", 0);
    variant_count.insert("MouseScroll", 0);

    let mut totals: HashMap<&'static str, usize> = HashMap::new();
    totals.insert("Keyboard", 0);
    totals.insert("MouseButton", 0);
    totals.insert("MouseScroll", 0);

    while let Ok(event) = rx.recv().await {
        let variant = event.variant();
        let count = variant_count.get_mut(&variant).unwrap();
        if &*count <= &buffer_size {
            event_buffer.push(event);
        }
        *count += 1;

        *totals.get_mut(&variant).unwrap() += 1;

        // remove the first event in buffer with same variant as current event
        if let Some(first_variant_index) = event_buffer.iter().position(|event| variant == event.variant()) {
            if &*count >= &(buffer_size + 1) {
                event_buffer.remove(first_variant_index);
                *count -= 1;
            }
        }

        // render template in advance to avoid unnecessary waiting
        let template = template::All {
            events: event_buffer.clone(),
            buffer_size,
            event_totals: totals.clone(),
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
