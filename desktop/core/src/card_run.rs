// Run on the card (docs/protocol.md, "Run on the card"; RowRenderer.appendRun):
// what an ask's Run button says, from its button as the app last heard of it.
// The GPUI app's card_run.rs draws it and presses it.
use crate::reducer::AskRun;
use crate::reply_buttons::ReplyButton;

/// Where a run is: Run, Run again after a failure, Running (pressed and on
/// its way, or the runner says it's going), and Ran once it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    Run,
    RunAgain,
    Running,
    Ran,
}

impl RunState {
    /// What the button says.
    pub fn title(self) -> &'static str {
        match self {
            RunState::Run => "▶ Run",
            RunState::RunAgain => "▶ Run again",
            RunState::Running => "Running…",
            RunState::Ran => "✓ Ran",
        }
    }

    /// Whether a press would run it: not while it's going, and never twice.
    pub fn can_press(self) -> bool {
        matches!(self, RunState::Run | RunState::RunAgain)
    }
}

/// The run's state and why it last failed. `button` is its button as last
/// heard of, if it has been; without one the ask's own mirror of it says
/// (`running`, `failed`), as the phone reads it. `pressing` is a press of
/// this app's still on its way.
pub fn run_state(run: &AskRun, button: Option<&ReplyButton>, pressing: bool) -> (RunState, Option<String>) {
    let failed = match button {
        Some(b) => b.failed.as_ref().map(|f| f.error.clone()),
        None => run.failed.clone(),
    };
    let state = if button.is_some_and(|b| b.pressed.is_some()) {
        RunState::Ran
    } else if pressing || button.is_some_and(|b| b.running.is_some()) || (button.is_none() && run.running == Some(true)) {
        RunState::Running
    } else if failed.is_some() {
        RunState::RunAgain
    } else {
        RunState::Run
    };
    (state, failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reply_buttons::{At, Failed, Pressed};

    fn run() -> AskRun {
        AskRun { command: "npm run migrate".into(), button_id: Some("b7k2abcd".into()), cwd: Some("/w".into()), running: None, failed: None }
    }

    fn button() -> ReplyButton {
        ReplyButton { id: "b7k2abcd".into(), title: "Run".into(), ..Default::default() }
    }

    #[test]
    fn a_run_goes_from_run_to_running_to_ran() {
        assert_eq!(run_state(&run(), Some(&button()), false), (RunState::Run, None));
        assert_eq!(run_state(&run(), Some(&button()), true).0, RunState::Running, "pressed, the runner not answered yet");
        let going = ReplyButton { running: Some(At { at: "2026-11-11T10:00:00Z".into() }), ..button() };
        assert_eq!(run_state(&run(), Some(&going), false).0, RunState::Running);
        assert!(!RunState::Running.can_press());
        let done = ReplyButton { pressed: Some(Pressed { at: "2026-11-11T10:00:02Z".into(), thread_id: "t1".into() }), ..button() };
        assert_eq!(run_state(&run(), Some(&done), false).0, RunState::Ran);
        assert!(!RunState::Ran.can_press());
    }

    #[test]
    fn a_failed_run_says_why_and_can_go_again() {
        let failed = ReplyButton { failed: Some(Failed { at: "2026-11-11T10:00:00Z".into(), error: "The folder has gone.".into() }), ..button() };
        assert_eq!(run_state(&run(), Some(&failed), false), (RunState::RunAgain, Some("The folder has gone.".into())));
        assert!(RunState::RunAgain.can_press());
    }

    #[test]
    fn an_asks_command_comes_with_its_button_and_folder() {
        use crate::reducer::Ask;
        use serde_json::json;
        let ask = Ask::decode(&json!({ "action": "Backfill the titles on prod?", "run": { "command": "npx convex run --prod migrations:backfill", "cwd": "/w", "buttonId": "b7k2abcd", "running": true } })).unwrap();
        assert_eq!(ask.run, Some(AskRun { command: "npx convex run --prod migrations:backfill".into(), button_id: Some("b7k2abcd".into()), cwd: Some("/w".into()), running: Some(true), failed: None }));
        // No command, no run; and an ask without one is as it was.
        assert_eq!(Ask::decode(&json!({ "action": "Go", "run": { "cwd": "/w" } })).unwrap().run, None);
        assert_eq!(Ask::decode(&json!({ "action": "Go" })).unwrap().run, None);
    }

    #[test]
    fn with_no_button_heard_of_the_ask_says() {
        let mirrored = AskRun { running: Some(true), ..run() };
        assert_eq!(run_state(&mirrored, None, false).0, RunState::Running);
        let failed = AskRun { failed: Some("Cut off by a restart: it may have partly run.".into()), ..run() };
        assert_eq!(run_state(&failed, None, false).0, RunState::RunAgain);
        // A button heard of since outranks the ask's mirror.
        assert_eq!(run_state(&mirrored, Some(&button()), false).0, RunState::Run);
    }
}
