//! The UI adapter against a live AT-SPI session and real apps. Ignored by
//! default; run it on a session made for testing, such as the FRIDAY OS test
//! VM, with the accessibility status on (`busctl --user set-property
//! org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true`), one Nautilus
//! window and one pavucontrol window open (and, for the typing test, a Qt
//! `kdialog --inputbox Name kept` started with QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1):
//!
//! ```text
//! FRIDAY_TEST_REAL_ATSPI=1 cargo test -p friday-actions --test real_atspi -- --ignored
//! ```

#[cfg(target_os = "linux")]
mod live {
    use friday_actions::{ActionAdapter, ActionRequest, AtspiBackend, UiAdapter, UiBackend};
    use serde_json::{json, Value};

    fn enabled() -> bool {
        let on = std::env::var("FRIDAY_TEST_REAL_ATSPI").as_deref() == Ok("1");
        if !on {
            eprintln!("set FRIDAY_TEST_REAL_ATSPI=1 to run this against the live session");
        }
        on
    }

    async fn ui(a: &UiAdapter, verb: &str, params: Value) -> Value {
        let r = a.execute(ActionRequest::new("ui", verb, params)).await;
        assert!(r.success, "{verb} failed: {:?}", r.error);
        r.output
    }

    /// The states of the element at `path` (child indexes from the app root).
    async fn states_at(app: &str, path: &str) -> Vec<String> {
        let b = AtspiBackend::new();
        let apps = b.list_applications().await.unwrap();
        let mut id = apps
            .iter()
            .find(|a| a.name == app)
            .expect("app on the bus")
            .id
            .clone();
        let mut node = None;
        for idx in path.split('/') {
            let kids = b.get_children(&id).await.unwrap();
            let k = kids[idx.parse::<usize>().unwrap()].clone();
            id = k.id.clone();
            node = Some(k);
        }
        node.unwrap().states
    }

    /// GTK4 (Nautilus): it can be read, its buttons found by role word, deep
    /// under the unnamed panels GTK4 nests them in. Its buttons offer no press
    /// action, and the adapter says so.
    #[tokio::test]
    #[ignore = "reads a live GTK4 app over the accessibility bus"]
    async fn read_a_gtk4_app_and_say_plainly_what_cannot_be_pressed() {
        if !enabled() {
            return;
        }
        let a = UiAdapter::new();
        let apps = ui(&a, "apps", json!({})).await;
        assert!(
            apps.as_array()
                .unwrap()
                .iter()
                .any(|x| x["name"] == "org.gnome.Nautilus"),
            "Nautilus isn't on the accessibility bus (status on? a window open?): {apps}"
        );
        let buttons = ui(
            &a,
            "find",
            json!({"app": "nautilus", "name": "", "role": "button"}),
        )
        .await;
        let names: Vec<&str> = buttons
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["name"].as_str())
            .collect();
        assert!(
            names.contains(&"Back") && names.contains(&"Forward"),
            "{names:?}"
        );
        let r = a
            .execute(ActionRequest::new(
                "ui",
                "click",
                json!({"app": "nautilus", "name": "Back"}),
            ))
            .await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("offers no way to press it"));
    }

    /// GTK3 (pavucontrol) implements the Action interface: a tab is pressed by
    /// name and becomes the selected one.
    #[tokio::test]
    #[ignore = "drives a live GTK3 app over the accessibility bus"]
    async fn press_a_tab_in_a_gtk3_app_and_see_it_selected() {
        if !enabled() {
            return;
        }
        let a = UiAdapter::new();
        for tab in ["Configuration", "Playback"] {
            let clicked = ui(&a, "click", json!({"app": "pavucontrol", "name": tab})).await;
            assert_eq!(clicked["clicked"], tab);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let path = clicked["path"].as_str().unwrap().to_string();
            let states = states_at("pavucontrol", &path).await;
            assert!(
                states.iter().any(|s| s == "Selected"),
                "{tab} wasn't selected: {states:?}"
            );
        }
    }

    /// Qt 6 (kdialog, started with QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 as
    /// `kdialog --inputbox Name kept`): typed text goes in at the caret, and
    /// the text already in the field stays.
    #[tokio::test]
    #[ignore = "types into a live Qt app over the accessibility bus"]
    async fn type_into_a_qt_field_and_keep_its_text() {
        if !enabled() {
            return;
        }
        let a = UiAdapter::new();
        let found = ui(
            &a,
            "find",
            json!({"app": "kdialog", "name": "", "role": "text"}),
        )
        .await;
        let path = found[0]["path"].as_str().expect("a text field").to_string();
        ui(
            &a,
            "type_text",
            json!({"app": "kdialog", "path": path, "text": " plus"}),
        )
        .await;
        let after = ui(&a, "read", json!({"app": "kdialog", "path": path}))
            .await
            .to_string();
        assert!(after.contains("kept") && after.contains(" plus"), "{after}");
    }
}
