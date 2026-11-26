//! Audio focus and unplug behaviour. Android reports what happened; this decides what Loss does.
//!
//! * A call (temporary focus loss) pauses, and playback resumes when it ends, unless the user
//!   paused or played in the meantime.
//! * Another app taking over for good pauses, with no resume.
//! * When Android refuses focus (e.g. pressing play during a call), Loss doesn't play over it.
//! * Headphones, the USB-C DAC or Bluetooth disconnecting pauses instead of switching to the
//!   speaker.
//! * (M2-17) Muting the phone pauses and raising the volume again resumes, if the user wants.
//!   Connecting headphones, a DAC or a car's Bluetooth can resume playback: never, only after
//!   one of them disconnected and paused it, or always.

use crate::event::{is_personal_output, Event, FocusChange, MediaCommand};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    #[default]
    Nothing,
    Pause,
    Resume,
}

/// When connecting an output starts the music again.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOnConnect {
    #[default]
    Off,
    /// Only when its disconnecting is what paused the music.
    AfterDisconnect,
    Always,
}

impl ResumeOnConnect {
    pub fn parse(s: &str) -> ResumeOnConnect {
        match s {
            "after_disconnect" => ResumeOnConnect::AfterDisconnect,
            "always" => ResumeOnConnect::Always,
            _ => ResumeOnConnect::Off,
        }
    }
}

/// The user's choices.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Prefs {
    pub pause_on_mute: bool,
    pub resume_on_connect: ResumeOnConnect,
}

/// Whether raising the volume from `current` to `target` (both 0..1) needs the user's OK:
/// `limit` is the percentage above which it does (0 turns the question off). Lowering never
/// asks, and neither does staying under the limit.
pub fn needs_loud_confirmation(current: f32, target: f32, limit_percent: i64) -> bool {
    if limit_percent <= 0 {
        return false;
    }
    let limit = limit_percent as f32 / 100.0;
    target > limit + 1e-4 && target > current + 1e-4 && current <= limit + 1e-4
}

#[derive(Debug, Default)]
pub struct FocusPolicy {
    /// Paused by a temporary focus loss; resume when focus comes back.
    paused_for_focus: bool,
    /// Paused because an output disconnected.
    paused_by_unplug: bool,
    /// Paused because the volume went to zero.
    paused_by_mute: bool,
    prefs: Prefs,
}

impl FocusPolicy {
    pub const fn new() -> FocusPolicy {
        FocusPolicy {
            paused_for_focus: false,
            paused_by_unplug: false,
            paused_by_mute: false,
            prefs: Prefs {
                pause_on_mute: false,
                resume_on_connect: ResumeOnConnect::Off,
            },
        }
    }

    pub fn set_prefs(&mut self, prefs: Prefs) {
        self.prefs = prefs;
    }

    /// What to do about `event`, given whether music is playing right now. Events that aren't
    /// about focus or outputs give [`Reaction::Nothing`].
    pub fn on_event(&mut self, event: &Event, playing: bool) -> Reaction {
        match event {
            Event::Focus(change) => self.on_focus(*change, playing),
            Event::Noisy => self.unplugged(playing),
            Event::OutputsRemoved(types) if types.iter().any(|t| is_personal_output(*t)) => {
                self.unplugged(playing)
            }
            Event::Media(cmd) => {
                // The user decided; a pending resume after a call no longer applies.
                if matches!(
                    cmd,
                    MediaCommand::Play | MediaCommand::Pause | MediaCommand::Toggle
                ) {
                    self.paused_for_focus = false;
                    self.paused_by_unplug = false;
                    self.paused_by_mute = false;
                }
                Reaction::Nothing
            }
            Event::Volume { level, .. } => self.on_volume(*level, playing),
            Event::OutputsAdded(types) if types.iter().any(|t| is_personal_output(*t)) => {
                self.connected(playing)
            }
            _ => Reaction::Nothing,
        }
    }

    fn on_focus(&mut self, change: FocusChange, playing: bool) -> Reaction {
        match change {
            FocusChange::Loss | FocusChange::Refused => {
                self.paused_for_focus = false;
                if playing {
                    Reaction::Pause
                } else {
                    Reaction::Nothing
                }
            }
            FocusChange::LossTransient => {
                if playing {
                    self.paused_for_focus = true;
                    Reaction::Pause
                } else {
                    Reaction::Nothing
                }
            }
            FocusChange::Duck => Reaction::Nothing,
            FocusChange::Gain => {
                if std::mem::take(&mut self.paused_for_focus) && !playing {
                    Reaction::Resume
                } else {
                    Reaction::Nothing
                }
            }
        }
    }

    fn on_volume(&mut self, level: i32, playing: bool) -> Reaction {
        if !self.prefs.pause_on_mute {
            self.paused_by_mute = false;
            return Reaction::Nothing;
        }
        if level <= 0 {
            if playing {
                self.paused_by_mute = true;
                return Reaction::Pause;
            }
        } else if std::mem::take(&mut self.paused_by_mute) && !playing {
            return Reaction::Resume;
        }
        Reaction::Nothing
    }

    fn connected(&mut self, playing: bool) -> Reaction {
        let after_unplug = std::mem::take(&mut self.paused_by_unplug);
        if playing {
            return Reaction::Nothing;
        }
        match self.prefs.resume_on_connect {
            ResumeOnConnect::Off => Reaction::Nothing,
            ResumeOnConnect::AfterDisconnect if !after_unplug => Reaction::Nothing,
            _ => Reaction::Resume,
        }
    }

    fn unplugged(&mut self, playing: bool) -> Reaction {
        self.paused_for_focus = false;
        // (Android sends both "noisy" and the removed output: don't forget the first.)
        self.paused_by_unplug = self.paused_by_unplug || playing;
        if playing {
            Reaction::Pause
        } else {
            Reaction::Nothing
        }
    }

    /// Call whenever the player's state is known; playing clears a pending resume.
    pub fn observe(&mut self, playing: bool) {
        if playing {
            self.paused_for_focus = false;
        }
    }

    /// Whether the glue should keep its focus request while paused (waiting for a call to end).
    pub fn keep_focus(&self) -> bool {
        self.paused_for_focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(c: FocusChange) -> Event {
        Event::Focus(c)
    }

    #[test]
    fn call_pauses_and_resumes() {
        let mut p = FocusPolicy::default();
        assert_eq!(
            p.on_event(&focus(FocusChange::LossTransient), true),
            Reaction::Pause
        );
        assert!(p.keep_focus());
        assert_eq!(
            p.on_event(&focus(FocusChange::Gain), false),
            Reaction::Resume
        );
        assert!(!p.keep_focus());
        // A second gain does nothing.
        assert_eq!(
            p.on_event(&focus(FocusChange::Gain), false),
            Reaction::Nothing
        );
    }

    #[test]
    fn no_resume_if_it_was_not_playing() {
        let mut p = FocusPolicy::default();
        assert_eq!(
            p.on_event(&focus(FocusChange::LossTransient), false),
            Reaction::Nothing
        );
        assert_eq!(
            p.on_event(&focus(FocusChange::Gain), false),
            Reaction::Nothing
        );
    }

    #[test]
    fn user_pause_during_call_cancels_resume() {
        let mut p = FocusPolicy::default();
        p.on_event(&focus(FocusChange::LossTransient), true);
        p.on_event(&Event::Media(MediaCommand::Pause), false);
        assert_eq!(
            p.on_event(&focus(FocusChange::Gain), false),
            Reaction::Nothing
        );
    }

    #[test]
    fn permanent_loss_pauses_without_resume() {
        let mut p = FocusPolicy::default();
        assert_eq!(p.on_event(&focus(FocusChange::Loss), true), Reaction::Pause);
        assert_eq!(
            p.on_event(&focus(FocusChange::Gain), false),
            Reaction::Nothing
        );
    }

    #[test]
    fn refused_focus_stops_playback() {
        let mut p = FocusPolicy::default();
        assert_eq!(
            p.on_event(&focus(FocusChange::Refused), true),
            Reaction::Pause
        );
    }

    #[test]
    fn duck_does_nothing() {
        let mut p = FocusPolicy::default();
        assert_eq!(
            p.on_event(&focus(FocusChange::Duck), true),
            Reaction::Nothing
        );
    }

    #[test]
    fn unplugging_headphones_pauses() {
        let mut p = FocusPolicy::default();
        assert_eq!(p.on_event(&Event::Noisy, true), Reaction::Pause);
        assert_eq!(
            p.on_event(&Event::OutputsRemoved(vec![22]), true),
            Reaction::Pause
        );
        // The speaker going away (it doesn't, but) or an HDMI sink: keep playing.
        assert_eq!(
            p.on_event(&Event::OutputsRemoved(vec![2, 9]), true),
            Reaction::Nothing
        );
        assert_eq!(p.on_event(&Event::Noisy, false), Reaction::Nothing);
    }

    fn prefs(mute: bool, connect: ResumeOnConnect) -> FocusPolicy {
        let mut p = FocusPolicy::default();
        p.set_prefs(Prefs {
            pause_on_mute: mute,
            resume_on_connect: connect,
        });
        p
    }

    fn volume(level: i32) -> Event {
        Event::Volume { level, max: 15 }
    }

    #[test]
    fn muting_pauses_and_raising_the_volume_resumes() {
        let mut p = prefs(true, ResumeOnConnect::Off);
        assert_eq!(p.on_event(&volume(0), true), Reaction::Pause);
        assert_eq!(p.on_event(&volume(0), false), Reaction::Nothing);
        assert_eq!(p.on_event(&volume(3), false), Reaction::Resume);
        assert_eq!(p.on_event(&volume(4), false), Reaction::Nothing);
        // Not when the user paused (or started) it by hand while it was muted.
        assert_eq!(p.on_event(&volume(0), true), Reaction::Pause);
        p.on_event(&Event::Media(MediaCommand::Pause), false);
        assert_eq!(p.on_event(&volume(5), false), Reaction::Nothing);
        // Not when it was already paused at zero volume.
        assert_eq!(p.on_event(&volume(0), false), Reaction::Nothing);
        assert_eq!(p.on_event(&volume(5), false), Reaction::Nothing);
        // Off: nothing happens.
        let mut off = prefs(false, ResumeOnConnect::Off);
        assert_eq!(off.on_event(&volume(0), true), Reaction::Nothing);
    }

    #[test]
    fn connecting_resumes_as_chosen() {
        let plug = Event::OutputsAdded(vec![22]);
        // Off.
        let mut p = prefs(false, ResumeOnConnect::Off);
        p.on_event(&Event::Noisy, true);
        assert_eq!(p.on_event(&plug, false), Reaction::Nothing);
        // After a disconnect: only if unplugging paused it.
        let mut p = prefs(false, ResumeOnConnect::AfterDisconnect);
        assert_eq!(p.on_event(&plug, false), Reaction::Nothing);
        p.on_event(&Event::Noisy, true);
        p.on_event(&Event::OutputsRemoved(vec![22]), false);
        assert_eq!(p.on_event(&plug, false), Reaction::Resume);
        assert_eq!(p.on_event(&plug, false), Reaction::Nothing);
        // The user pressing pause after the unplug cancels it.
        p.on_event(&Event::Noisy, true);
        p.on_event(&Event::Media(MediaCommand::Pause), false);
        assert_eq!(p.on_event(&plug, false), Reaction::Nothing);
        // Always: whenever it isn't playing; never over music that is.
        let mut p = prefs(false, ResumeOnConnect::Always);
        assert_eq!(p.on_event(&plug, false), Reaction::Resume);
        assert_eq!(p.on_event(&plug, true), Reaction::Nothing);
        // The phone speaker or an HDMI sink isn't "headphones or a car".
        assert_eq!(
            p.on_event(&Event::OutputsAdded(vec![2, 9]), false),
            Reaction::Nothing
        );
    }

    #[test]
    fn loud_volume_asks_only_when_crossing_the_limit_upwards() {
        assert!(needs_loud_confirmation(0.5, 0.9, 80));
        assert!(needs_loud_confirmation(0.8, 0.85, 80)); // sitting on the limit, going over
        assert!(!needs_loud_confirmation(0.9, 1.0, 80)); // already above: confirmed before
        assert!(!needs_loud_confirmation(0.9, 0.7, 80)); // down
        assert!(!needs_loud_confirmation(0.5, 0.8, 80)); // not over
        assert!(!needs_loud_confirmation(0.5, 1.0, 0)); // off
    }

    #[test]
    fn unplug_during_call_cancels_resume() {
        let mut p = FocusPolicy::default();
        p.on_event(&focus(FocusChange::LossTransient), true);
        p.on_event(&Event::OutputsRemoved(vec![22]), false);
        assert_eq!(
            p.on_event(&focus(FocusChange::Gain), false),
            Reaction::Nothing
        );
    }

    #[test]
    fn playing_clears_pending_resume() {
        let mut p = FocusPolicy::default();
        p.on_event(&focus(FocusChange::LossTransient), true);
        p.observe(true);
        assert!(!p.keep_focus());
    }
}
