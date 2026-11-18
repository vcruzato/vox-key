use anyhow::{Context, Result};
use clap::Parser;
use ksni::blocking::TrayMethods;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing_subscriber::EnvFilter;

use voxkey::audio::Recorder;
use voxkey::config::{default_config_path, Config};
use voxkey::latch::{Action, Event, Latch, Mode};
use voxkey::output::Output;
use voxkey::ptt::{self, PttEvent};
use voxkey::single_instance;
use voxkey::transcriber::{Transcriber, WhisperCpp};
use voxkey::tray::{fmt_mmss, State, VoxTray};

/// How often we wake while recording to refresh the counter/blink and check the
/// auto-stop limit.
const POLL: Duration = Duration::from_millis(500);

#[derive(Parser, Debug)]
#[command(name = "voxkey", version, about = "Push-to-talk voice dictation for Linux")]
struct Cli {
    /// Path to config. Defaults to $XDG_CONFIG_HOME/voxkey/config.toml.
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Validate config and exit.
    #[arg(long)]
    check: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(default_config_path);
    let cfg =
        Config::load(&config_path).with_context(|| format!("loading {}", config_path.display()))?;

    if cli.check {
        println!(
            "OK (ptt_key={}, model={}, language={}, input_device={})",
            cfg.ptt_key, cfg.model, cfg.language, cfg.input_device
        );
        return Ok(());
    }

    let _guard = match single_instance::acquire(&config_path.display().to_string()) {
        Ok(l) => Some(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::error!("another voxkey instance is already running; exiting");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not acquire single-instance lock; continuing");
            None
        }
    };

    let ptt_key = ptt::key_from_name(&cfg.ptt_key)
        .with_context(|| format!("unknown ptt_key '{}'", cfg.ptt_key))?;

    tracing::info!(model = %cfg.model, "loading whisper model…");
    let mut transcriber = WhisperCpp::load(&cfg.resolved_model_path(), &cfg.language)
        .context("loading whisper model")?;

    let mut output = Output::new()?;
    let mut recorder = Recorder::new(&cfg.input_device, cfg.max_seconds)?;

    let paused = Arc::new(AtomicBool::new(false));
    let tray = VoxTray {
        state: State::Idle,
        paused: paused.clone(),
        elapsed_s: 0,
        total_s: cfg.max_seconds,
        blink_on: false,
    };
    let tray_handle = tray
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawning tray: {e}"))?;

    let rx = ptt::watch(ptt_key)?;
    tracing::info!(
        key = %cfg.ptt_key,
        hold_ms = cfg.hold_to_start_ms,
        "voxkey running — segure a tecla para iniciar; toque para parar. ctrl-c encerra."
    );

    let hold = Duration::from_millis(cfg.hold_to_start_ms);
    let limit = Duration::from_secs(cfg.max_seconds as u64);
    // Blink window can't exceed the recording length.
    let warn = Duration::from_secs(cfg.warn_seconds.min(cfg.max_seconds) as u64);

    let mut latch = Latch::new();
    let mut arm_deadline: Option<Instant> = None; // when the hold completes (Arming)
    let mut rec_start: Option<Instant> = None; // when recording began
    let mut blink_on = false;
    let mut shown_s: i64 = -1; // last second rendered, to skip redundant tray updates

    loop {
        let timeout = match latch.mode() {
            Mode::Idle => None,
            Mode::Arming => Some(
                arm_deadline.map_or(Duration::ZERO, |d| d.saturating_duration_since(Instant::now())),
            ),
            Mode::Recording => Some(POLL),
        };

        let event = match timeout {
            None => match rx.recv() {
                Ok(ev) => Some(ev),
                Err(_) => break, // channel closed
            },
            Some(t) => match rx.recv_timeout(t) {
                Ok(ev) => Some(ev),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
        };

        // Translate the real PTT event / timeout into a latch Event.
        let latch_event = match (latch.mode(), event) {
            (mode, Some(PttEvent::Press)) => {
                if mode == Mode::Idle && paused.load(Ordering::Relaxed) {
                    continue; // paused: ignore start
                }
                Event::Press
            }
            (_, Some(PttEvent::Release)) => Event::Release,
            (Mode::Arming, None) => Event::HoldElapsed,
            (Mode::Recording, None) => {
                let elapsed = rec_start.map_or(Duration::ZERO, |s| s.elapsed());
                if elapsed >= limit {
                    Event::LimitReached
                } else {
                    let secs = elapsed.as_secs();
                    let in_warn = elapsed + warn >= limit;
                    let next_blink = in_warn && !blink_on;
                    if secs as i64 != shown_s || next_blink != blink_on {
                        blink_on = next_blink;
                        shown_s = secs as i64;
                        tray_handle.update(move |t: &mut VoxTray| {
                            t.elapsed_s = secs as u32;
                            t.blink_on = next_blink;
                        });
                    }
                    continue;
                }
            }
            (_, None) => continue, // no timeout fires in Idle
        };

        let prev = latch.mode();
        let action = latch.on(latch_event);
        let now_mode = latch.mode();
        if now_mode == Mode::Arming && prev != Mode::Arming {
            arm_deadline = Some(Instant::now() + hold);
        } else if now_mode != Mode::Arming {
            arm_deadline = None;
        }

        match action {
            Action::Nothing => {}
            Action::Start => match recorder.start() {
                Ok(()) => {
                    rec_start = Some(Instant::now());
                    blink_on = false;
                    shown_s = -1;
                    tray_handle.update(|t: &mut VoxTray| {
                        t.state = State::Recording;
                        t.elapsed_s = 0;
                        t.blink_on = false;
                    });
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start recording");
                    latch = Latch::new();
                    arm_deadline = None;
                    tray_handle.update(|t: &mut VoxTray| t.state = State::Error);
                }
            },
            Action::Stop => {
                let by_limit = latch_event == Event::LimitReached;
                let pcm = recorder.stop();
                rec_start = None;
                let audio_s = pcm.len() as f32 / 16_000.0;
                if by_limit {
                    voxkey::notify::notify(
                        "voxkey",
                        &format!("Limite de {} atingido — transcrevendo", fmt_mmss(cfg.max_seconds)),
                    );
                }
                tray_handle.update(|t: &mut VoxTray| t.state = State::Transcribing);
                let t0 = Instant::now();
                let result = transcriber.transcribe(&pcm);
                let took_ms = t0.elapsed().as_millis();
                match result {
                    Ok(text) if !text.trim().is_empty() => {
                        let text = text.trim();
                        tracing::info!(audio_s, took_ms, chars = text.len(), "transcribed");
                        if let Err(e) = output.paste_text(text) {
                            tracing::error!(error = %e, "failed to paste");
                        }
                    }
                    Ok(_) => {
                        tracing::info!(audio_s, took_ms, "empty transcription — nothing to paste")
                    }
                    Err(e) => tracing::error!(error = %e, "transcription failed"),
                }
                tray_handle.update(|t: &mut VoxTray| t.state = State::Idle);
            }
        }
    }
    Ok(())
}
