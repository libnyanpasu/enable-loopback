//! Routed WinUI key handlers; top-level HWND keyboard messages do not reach XAML.
use super::Message;
use std::{cell::Cell, rc::Rc};
use windows_reactor::*;

pub(super) fn list(
    sender: LocalSender<Message>,
    checkbox_focus: Rc<Cell<bool>>,
) -> RoutedCallback<KeyEventInfo> {
    RoutedCallback::new(move |event: KeyEventInfo| {
        let control = event.modifiers.contains(InputModifiers::CONTROL);
        let alt = event.modifiers.contains(InputModifiers::ALT);
        let handled = !alt
            && ((control && (event.key == VirtualKey::A || event.key == VirtualKey::C))
                || (!control && (event.key == VirtualKey::UP || event.key == VirtualKey::DOWN))
                || (!control && event.key == VirtualKey::SPACE && !checkbox_focus.get()));
        handled && sender.send(Message::ListKey(event))
    })
}
