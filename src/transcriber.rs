use anyhow::{anyhow, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Engine-agnostic transcription. `pcm_16k_mono` is 16 kHz mono f32.
pub trait Transcriber {
    fn transcribe(&mut self, pcm_16k_mono: &[f32]) -> Result<String>;
}

/// Test double — returns a canned reply and records the last input length.
pub struct FakeTranscriber {
    pub reply: String,
    pub last_len: usize,
}

impl Transcriber for FakeTranscriber {
    fn transcribe(&mut self, pcm_16k_mono: &[f32]) -> Result<String> {
        self.last_len = pcm_16k_mono.len();
        Ok(self.reply.clone())
    }
}

/// whisper.cpp engine (CPU, greedy, fixed language, no timestamps).
pub struct WhisperCpp {
    ctx: WhisperContext,
    language: String,
    n_threads: i32,
}

impl WhisperCpp {
    pub fn load(model_path: &Path, language: &str) -> Result<Self> {
        if !model_path.exists() {
            anyhow::bail!(
                "model file missing: {}\nDownload it once with:  scripts/fetch-model.sh <model>",
                model_path.display()
            );
        }
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| anyhow!("loading whisper model {}: {e}", model_path.display()))?;
        // Cap at 8 threads: past the physical cores, more threads hurt (memory bandwidth).
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4) as i32;
        Ok(Self {
            ctx,
            language: language.to_string(),
            n_threads,
        })
    }
}

impl Transcriber for WhisperCpp {
    fn transcribe(&mut self, pcm_16k_mono: &[f32]) -> Result<String> {
        if pcm_16k_mono.is_empty() {
            return Ok(String::new());
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| anyhow!("creating whisper state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
        params.set_n_threads(self.n_threads);
        params.set_translate(false);
        params.set_language(Some(&self.language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, pcm_16k_mono)
            .map_err(|e| anyhow!("running whisper: {e}"))?;

        let mut out = String::new();
        for segment in state.as_iter() {
            let text = segment
                .to_str_lossy()
                .map_err(|e| anyhow!("reading segment text: {e}"))?;
            out.push_str(&text);
        }
        Ok(out.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_returns_canned_reply_and_sees_input() {
        let mut t = FakeTranscriber {
            reply: "olá mundo".into(),
            last_len: 0,
        };
        let out = t.transcribe(&[0.0; 320]).unwrap();
        assert_eq!(out, "olá mundo");
        assert_eq!(t.last_len, 320);
    }
}
