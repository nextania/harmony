mod shader;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use arc_swap::ArcSwap;
use async_stream::stream;
use iced::widget::{column, text};
use iced::{Element, Length, Subscription, Task};
use tracing::debug;

use wgpu_capture::{
    CaptureFrame, CaptureTarget, Codec, EncodeConfig, EncodeOutput, create_capturer, create_encoder,
};

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("Screen Viewer")
        .subscription(App::subscription)
        .run()
}

struct App {
    latest_frame: Arc<ArcSwap<Option<CaptureFrame>>>,
    frame_count: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let latest_frame: Arc<ArcSwap<Option<CaptureFrame>>> =
            Arc::new(ArcSwap::from_pointee(None));
        let frame_count = Arc::new(AtomicU64::new(0));

        let (tick_tx, mut tick_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let latest_for_thread = latest_frame.clone();
        let count_for_thread = frame_count.clone();

        std::thread::spawn(move || {
            let mut capturer = match create_capturer(CaptureTarget::Monitor(0)) {
                Ok(c) => c,
                Err(e) => {
                    debug!("create_capturer: {e}");
                    return;
                }
            };

            if let Err(e) = capturer.start() {
                debug!("capturer.start: {e}");
                return;
            }

            // Encoder: read first frame to get dimensions, then initialise.
            let first_frame = loop {
                match capturer.next_frame() {
                    Some(f) => break f,
                    None => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            };
            let (w, h) = (first_frame.width(), first_frame.height());

            let encoder_config = EncodeConfig {
                width: w,
                height: h,
                fps: 60,
                bitrate_bps: 8_000_000,
                codec: Codec::H264,
                output: EncodeOutput::new(|dat| {
                    // In production: RTP-packetize and send over network.
                    debug!("encoded {} bytes", dat.len());
                }),
            };

            let mut encoder = match create_encoder(encoder_config) {
                Ok(e) => Some(e),
                Err(e) => {
                    debug!("create_encoder: {e}");
                    None
                }
            };

            if let Some(enc) = encoder.as_mut() {
                if let Err(e) = enc.submit_frame(&first_frame) {
                    debug!("submit_frame: {e}");
                }
            }
            latest_for_thread.store(Arc::new(Some(first_frame)));
            count_for_thread.fetch_add(1, Ordering::Relaxed);
            tick_tx.send(()).ok();

            loop {
                match capturer.next_frame() {
                    Some(frame) => {
                        if let Some(enc) = encoder.as_mut() {
                            if let Err(e) = enc.submit_frame(&frame) {
                                debug!("submit_frame: {e}");
                            }
                        }
                        latest_for_thread.store(Arc::new(Some(frame)));
                        count_for_thread.fetch_add(1, Ordering::Relaxed);
                        tick_tx.send(()).ok();
                    }
                    None => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        });

        (
            App {
                latest_frame,
                frame_count,
            },
            Task::stream(stream! {
                loop {
                    match tick_rx.recv().await {
                        Some(()) => yield Message::Tick,
                        None => break,
                    }
                }
            }),
        )
    }

    fn update(&mut self, _message: Message) -> Task<Message> {
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        // iced::time::every(Duration::from_millis(33))
        //     .map(|_| Message::Tick)
        iced::Subscription::none()
    }

    fn view<'a>(&'a self) -> Element<'a, Message> {
        let frame_ref = Arc::clone(&self.latest_frame);

        column![
            text(format!(
                "Frames received: {}",
                self.frame_count.load(Ordering::Relaxed)
            )),
            iced::widget::shader(shader::ScreenProgram::new(frame_ref))
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .into()
    }
}
