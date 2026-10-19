//! Render the actual component through reactor's recording adapter. Background closures
//! are retained, never run: these tests cannot enumerate/write the real firewall table.
use super::*;
use std::sync::{Arc, Mutex};
use windows::Win32::winerror::{ERROR_ACCESS_DENIED, ERROR_INVALID_DATA};

type Timer = (Arc<TimerRegistration>, Box<dyn FnOnce() + Send>);
#[derive(Default)]
struct Services {
    background: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    timers: Mutex<VecDeque<Timer>>,
}
#[derive(Default)]
struct TimerRegistration;
impl ComponentTimerRegistration for TimerRegistration {
    fn cancel(&self) {}
}
impl ComponentServices for Services {
    fn spawn_background(&self, work: Box<dyn FnOnce() + Send>) {
        self.background.lock().unwrap().push(work);
    }
    fn set_timeout(
        &self,
        _: Duration,
        callback: Box<dyn FnOnce() + Send>,
    ) -> Arc<dyn ComponentTimerRegistration> {
        let timer = Arc::new(TimerRegistration);
        self.timers
            .lock()
            .unwrap()
            .push_back((timer.clone(), callback));
        timer
    }
}
struct Fixture {
    host: ComponentHost<RecordingAdapter>,
    sender: ComponentSender<Message>,
    services: Arc<Services>,
    _primary: crate::instance::Primary,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let primary = crate::instance::acquire_for_test(name);
        let services = Arc::new(Services::default());
        let host = ComponentHost::mount_with_services(
            RecordingAdapter::default(),
            services.clone(),
            [component::<MainWindow>(
                "main",
                Startup {
                    options: GuiOptions {
                        manual: true,
                        registry: false,
                    },
                    publisher: primary.publisher(),
                },
            )],
        )
        .unwrap();
        let sender = host.sender::<MainWindow>(&Key::from("main")).unwrap();
        Self {
            host,
            sender,
            services,
            _primary: primary,
        }
    }
    fn send(&mut self, message: Message) {
        assert!(self.sender.send(message));
        self.host.drain(100).unwrap();
        loop {
            let timer = self.services.timers.lock().unwrap().pop_front();
            let Some((_, callback)) = timer else { break };
            callback();
            self.host.drain(100).unwrap();
        }
    }
    fn close_dialog(&mut self, result: ContentDialogResult) {
        self.send(Message::DialogClosed(result));
    }
    fn load(&mut self, orphans: &[u32]) {
        self.send(Message::Loaded {
            registry: false,
            force: false,
            fallback_code: None,
            result: Ok(Loaded {
                apps: vec![app(1), app(2)],
                exempted: orphans.iter().map(|id| sid(*id)).collect(),
                user: "DOMAIN\\user".into(),
            }),
        });
    }
    fn property(&self, id: &str, property: PropertyId) -> PropertyValue {
        let graph = self.host.runtime().graph();
        let object = find(graph, graph.root().unwrap(), id).unwrap();
        graph
            .properties(object)
            .unwrap()
            .iter()
            .find(|p| p.id == property)
            .unwrap()
            .value
            .clone()
    }
    fn dialog(&self) -> (ObjectId, bool) {
        let graph = self.host.runtime().graph();
        let root = find(graph, graph.root().unwrap(), "main").unwrap();
        self.host.adapter().content_dialog(root).unwrap()
    }
    fn dialog_property(&self, property: PropertyId) -> PropertyValue {
        let (dialog, _) = self.dialog();
        self.host
            .runtime()
            .graph()
            .properties(dialog)
            .unwrap()
            .iter()
            .find(|p| p.id == property)
            .unwrap()
            .value
            .clone()
    }
}
fn find(graph: &RetainedGraph, root: ObjectId, id: &str) -> Option<ObjectId> {
    if graph
        .properties(root)
        .unwrap()
        .iter()
        .any(|p| p.id == PropertyId::AutomationId && p.value == PropertyValue::String(id.into()))
    {
        return Some(root);
    }
    for relation in [
        RelationId::Content,
        RelationId::Children,
        RelationId::PrimaryCommands,
        RelationId::SecondaryCommands,
        RelationId::Items,
    ] {
        if let Some(child) = graph.child(root, relation)
            && let Some(found) = find(graph, child, id)
        {
            return Some(found);
        }
        for child in graph.children(root, relation).unwrap_or_default() {
            if let Some(found) = find(graph, *child, id) {
                return Some(found);
            }
        }
    }
    None
}
fn sid(id: u32) -> Sid {
    format!("S-1-15-2-{id}").parse().unwrap()
}
fn app(id: u32) -> AppContainer {
    AppContainer {
        sid: sid(id),
        users: vec!["DOMAIN\\user".into()],
        name: Some(format!("app{id}")),
        display_name: Some(format!("app{id}")),
        description: None,
        package_full_name: None,
        binaries: vec![],
        exempt_at_last_check: false,
    }
}

#[test]
fn actual_view_retains_dirty_edits_after_access_denied_and_commits_only_success() {
    let mut f = Fixture::new("save");
    f.load(&[]);
    assert_eq!(
        f.property("save", PropertyId::IsEnabled),
        PropertyValue::Bool(false)
    );
    f.send(Message::Checked(sid(1), true));
    f.send(Message::Save);
    assert_eq!(f.services.background.lock().unwrap().len(), 1);
    assert_eq!(
        f.property("save", PropertyId::IsEnabled),
        PropertyValue::Bool(false)
    );
    f.send(Message::Saved(Err(
        netiso::Error::SetExemptionsAccessDenied {
            sid_count: 1,
            code: ERROR_ACCESS_DENIED as u32,
        },
    )));
    assert!(f.dialog().1);
    assert_eq!(
        f.dialog_property(PropertyId::Title),
        PropertyValue::String(rust_i18n::t!("dialog.admin_title").into_owned().into())
    );
    assert_eq!(
        f.property("save", PropertyId::IsEnabled),
        PropertyValue::Bool(true)
    );
    assert_eq!(
        f.property("counter", PropertyId::Text),
        PropertyValue::String("1 / 2".into())
    );
    f.close_dialog(ContentDialogResult::None);
    f.send(Message::Save);
    assert_eq!(f.services.background.lock().unwrap().len(), 2);
    f.send(Message::Saved(Err(netiso::Error::SetExemptions {
        sid_count: 1,
        code: ERROR_INVALID_DATA as u32,
    })));
    assert_eq!(
        f.property("save", PropertyId::IsEnabled),
        PropertyValue::Bool(true)
    );
    assert_eq!(
        f.dialog_property(PropertyId::Title),
        PropertyValue::String(rust_i18n::t!("dialog.save_title").into_owned().into())
    );
    f.close_dialog(ContentDialogResult::None);
    f.send(Message::Save);
    assert_eq!(f.services.background.lock().unwrap().len(), 3);
    f.send(Message::Saved(Ok(())));
    assert_eq!(
        f.property("save", PropertyId::IsEnabled),
        PropertyValue::Bool(false)
    );
    assert_eq!(
        f.property("status", PropertyId::Text),
        PropertyValue::String(rust_i18n::t!("status.saved").into_owned().into())
    );
}

#[test]
fn confirmations_default_to_cancel_and_do_not_drop_edits() {
    let mut f = Fixture::new("confirm");
    f.load(&[]);
    f.send(Message::Checked(sid(1), true));
    for message in [Message::Refresh, Message::CloseRequested] {
        f.send(message);
        assert!(f.dialog().1);
        assert_eq!(
            f.dialog_property(PropertyId::DefaultButton),
            PropertyValue::Enum {
                kind: "ContentDialogButton",
                variant: "Close"
            }
        );
        f.close_dialog(ContentDialogResult::None);
        assert!(!f.dialog().1);
        assert_eq!(
            f.property("save", PropertyId::IsEnabled),
            PropertyValue::Bool(true)
        );
        assert!(f.services.background.lock().unwrap().is_empty());
    }
    f.send(Message::Refresh);
    f.close_dialog(ContentDialogResult::Primary);
    assert_eq!(f.services.background.lock().unwrap().len(), 1);
    assert_eq!(
        f.property("apps", PropertyId::IsEnabled),
        PropertyValue::Bool(false)
    );
}

#[test]
fn fallback_warning_precedes_registry_read_and_firewall_failure_dialog() {
    let mut f = Fixture::new("fallback");
    f.send(Message::BeginRefresh(false));
    assert_eq!(f.services.background.lock().unwrap().len(), 1);
    f.send(Message::Loaded {
        registry: false,
        force: false,
        fallback_code: None,
        result: Err(netiso::Error::Enumerate {
            force_binaries: false,
            code: EPT_S_NOT_REGISTERED as u32,
        }),
    });
    assert_eq!(
        f.dialog_property(PropertyId::Title),
        PropertyValue::String(rust_i18n::t!("dialog.warning").into_owned().into())
    );
    assert_eq!(f.services.background.lock().unwrap().len(), 1); // no fallback until acknowledged
    f.close_dialog(ContentDialogResult::None);
    assert_eq!(f.services.background.lock().unwrap().len(), 2);
    f.send(Message::Loaded {
        registry: true,
        force: false,
        fallback_code: Some(EPT_S_NOT_REGISTERED as u32),
        result: Err(netiso::Error::ReadRegistry {
            step: "open",
            path: "test".into(),
            code: ERROR_INVALID_DATA as u32,
        }),
    });
    assert_eq!(
        f.dialog_property(PropertyId::Title),
        PropertyValue::String(rust_i18n::t!("dialog.firewall_title").into_owned().into())
    );
    assert_eq!(
        f.property("refresh", PropertyId::IsEnabled),
        PropertyValue::Bool(true)
    );
    assert_eq!(
        f.property("apps", PropertyId::IsEnabled),
        PropertyValue::Bool(false)
    );
}

#[test]
fn orphan_prompts_follow_config_order_and_opt_out_lasts_for_the_session() {
    let mut f = Fixture::new("orphans");
    f.load(&[9, 8, 7]);
    assert!(f.dialog().1);
    let first = f.dialog().0;
    let content = f
        .host
        .runtime()
        .graph()
        .child(first, RelationId::Content)
        .unwrap();
    let children = f
        .host
        .runtime()
        .graph()
        .children(content, RelationId::Children)
        .unwrap();
    assert!(
        f.host
            .runtime()
            .graph()
            .properties(children[1])
            .unwrap()
            .iter()
            .any(|p| p.id == PropertyId::Text
                && p.value == PropertyValue::String(sid(9).to_string().into()))
    );
    f.close_dialog(ContentDialogResult::None);
    let second = f.dialog().0;
    let content = f
        .host
        .runtime()
        .graph()
        .child(second, RelationId::Content)
        .unwrap();
    let children = f
        .host
        .runtime()
        .graph()
        .children(content, RelationId::Children)
        .unwrap();
    assert!(
        f.host
            .runtime()
            .graph()
            .properties(children[1])
            .unwrap()
            .iter()
            .any(|p| p.id == PropertyId::Text
                && p.value == PropertyValue::String(sid(8).to_string().into()))
    );
    f.send(Message::KeepPrompting(false));
    f.close_dialog(ContentDialogResult::None);
    assert!(!f.dialog().1);
    f.load(&[9, 8]);
    assert!(!f.dialog().1);
    f.send(Message::Checked(sid(1), true));
    if let PropertyValue::String(status) = f.property("status", PropertyId::Text) {
        assert!(status.contains(rust_i18n::t!("status.orphan_removal", count = 2).as_ref()));
    } else {
        panic!("status text expected");
    }
}

#[test]
fn sorting_rebuilds_the_native_list_and_restores_the_sid_focus_after_mount() {
    let mut f = Fixture::new("sort");
    f.load(&[]);
    f.send(Message::Pointer(
        sid(1),
        Column::DisplayName,
        PointerEventInfo {
            is_left_button_pressed: true,
            ..Default::default()
        },
    ));
    f.send(Message::Sort(Column::DisplayName));
    assert_eq!(
        f.property("apps", PropertyId::SelectedIndex),
        PropertyValue::SelectionIndex(Some(1))
    );
    f.send(Message::Checked(sid(1), true));
    assert_eq!(
        f.property("counter", PropertyId::Text),
        PropertyValue::String("1 / 2".into())
    );
}
