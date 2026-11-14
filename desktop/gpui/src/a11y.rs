// An element's role for screen readers, as `.role(...)` sets it, except that
// `Role::GenericContainer` (what the source test, core/tests/a11y_roles_source.rs,
// asks of a wrapper that only names a kit control or some words for the
// app's scripts) sets nothing. A plain container gets no accessibility node
// either way, but gpui-pre's `role` asserts against being handed one, so in
// a debug or scripted build each of these panicked the app as it drew: the
// Terminal's "not the thread's folder" note took down a spec on the cloud box
// with nothing but "the app stopped" (13 Nov).
use gpui_kit::{Role, StatefulInteractiveElement};

pub(crate) trait A11yRole: StatefulInteractiveElement {
    fn a11y_role(self, role: Role) -> Self {
        if role == Role::GenericContainer { self } else { self.role(role) }
    }
}

impl<E: StatefulInteractiveElement> A11yRole for E {}

#[cfg(test)]
mod tests {
    use super::A11yRole as _;
    use gpui_kit::{div, InteractiveElement as _, Role};

    /// A plain container goes through without reaching gpui-pre's assertion.
    #[test]
    fn a_plain_container_doesnt_panic() {
        let _ = div().id("words").a11y_role(Role::GenericContainer);
        let _ = div().id("button").a11y_role(Role::Button);
    }
}
