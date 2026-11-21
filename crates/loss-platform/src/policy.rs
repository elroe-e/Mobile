//! Audio focus and unplug behaviour. Android reports what happened; this decides what Loss does.
//!
//! * A call (temporary focus loss) pauses, and playback resumes when it ends, unless the user
//!   paused or played in the meantime.
//! * Another app taking over for good pauses, with no resume.
//! * When Android refuses focus (e.g. pressing play during a call), Loss doesn't play over it.
//! * Headphones, the USB-C DAC or Bluetooth disconnecting pauses instead of switching to the
//!   speaker.

use crate::event::{is_personal_output, Event, FocusChange, MediaCommand};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    #[default]
    Nothing,
    Pause,
    Resume,
}

#[derive(Debug, Default)]
pub struct FocusPolicy {
    /// Paused by a temporary focus loss; resume when focus comes back.
    paused_for_focus: bool,
}

impl FocusPolicy {
    pub const fn new() -> FocusPolicy {
        FocusPolicy {
            paused_for_focus: false,
        }
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
                }
                Reaction::Nothing
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

    fn unplugged(&mut self, playing: bool) -> Reaction {
        self.paused_for_focus = false;
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
