//! The success and failure sounds, taken from the Speech Output Engine, and
//! the sound for a cancelled download.

use anyhow::Context;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

/// A short sound marking how something went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Success,
    Failure,
    Cancelled,
}

impl Cue {
    fn bytes(self) -> &'static [u8] {
        match self {
            Cue::Success => include_bytes!("../assets/sounds/success.mp3"),
            Cue::Failure => include_bytes!("../assets/sounds/failure.wav"),
            Cue::Cancelled => include_bytes!("../assets/sounds/cancel.wav"),
        }
    }
}

/// Plays cues, one at a time, each on its own thread so the window never
/// waits for the audio device.
#[derive(Default)]
pub struct Cues {
    current: Arc<Mutex<Playing>>,
}

#[derive(Default)]
struct Playing {
    /// How many cues have been started. A cue whose number is out of date by
    /// the time its audio device opens is not played, so a newer cue always
    /// wins.
    started: u64,
    /// The cue now playing, if any.
    player: Option<Arc<rodio::Player>>,
}

impl Cues {
    pub fn play(&self, cue: Cue) {
        let number = self.stop();
        let current = self.current.clone();
        let spawned = std::thread::Builder::new().name("sound".to_owned()).spawn(move || {
            if let Err(e) = play(cue, number, &current) {
                log::warn!("could not play the {cue:?} sound: {e:#}");
            }
        });
        if let Err(e) = spawned {
            log::warn!("could not start the sound thread: {e}");
        }
    }

    /// Cuts off the cue now playing or about to play. Returns the number the
    /// next cue should have.
    fn stop(&self) -> u64 {
        let mut playing = self.current.lock().unwrap_or_else(|e| e.into_inner());
        playing.started += 1;
        if let Some(player) = playing.player.take() {
            player.stop();
        }
        playing.started
    }
}

/// Plays `cue`, cue `number`, to the end, unless a newer one has started.
fn play(cue: Cue, number: u64, current: &Mutex<Playing>) -> anyhow::Result<()> {
    let mut device = rodio::DeviceSinkBuilder::open_default_sink().context("no audio output device is available")?;
    device.log_on_drop(false);
    let player = Arc::new(rodio::Player::connect_new(device.mixer()));
    let decoder = rodio::Decoder::try_from(Cursor::new(cue.bytes())).context("the sound is damaged")?;
    {
        let mut playing = current.lock().unwrap_or_else(|e| e.into_inner());
        if playing.started != number {
            return Ok(());
        }
        playing.player = Some(player.clone());
        player.append(decoder);
    }
    player.sleep_until_end();
    let mut playing = current.lock().unwrap_or_else(|e| e.into_inner());
    if playing.started == number {
        playing.player = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Cue;

    #[test]
    fn cues_decode() {
        for cue in [Cue::Success, Cue::Failure, Cue::Cancelled] {
            let decoder = rodio::Decoder::try_from(std::io::Cursor::new(cue.bytes())).unwrap();
            assert!(rodio::Source::total_duration(&decoder).is_none_or(|d| !d.is_zero()), "{cue:?}");
        }
    }
}
