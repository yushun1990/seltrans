//! One reusable native GTK/libadwaita window, owned by the resident process.

use std::rc::Rc;

use adw::prelude::*;
use adw::{glib, gtk};

use seltrans_core::classifier::classify_selection;
use seltrans_core::domain::SelectionMode;
use seltrans_core::ipc::Invocation;

pub fn run(receiver: async_channel::Receiver<Invocation>) {
    let app = adw::Application::builder()
        .application_id("io.github.yushun1990.seltrans")
        .build();
    let receiver = Rc::new(receiver);
    app.connect_activate(move |app| {
        // Repeated activation must not build another result window.
        if !app.windows().is_empty() {
            return;
        }
        let receiver = receiver.clone();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("seltrans")
            .default_width(600)
            .default_height(390)
            .build();
        window.set_hide_on_close(true);

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let quit = gtk::Button::with_label("Quit");
        let app_for_quit = app.clone();
        quit.connect_clicked(move |_| app_for_quit.quit());
        header.pack_end(&quit);
        toolbar.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(14)
            .margin_top(22)
            .margin_bottom(22)
            .margin_start(24)
            .margin_end(24)
            .build();

        let heading = gtk::Label::new(Some("Selection"));
        heading.set_xalign(0.0);
        heading.add_css_class("title-2");
        content.append(&heading);

        let mode = gtk::Label::new(Some("Select English text, then press Alt+Q."));
        mode.set_xalign(0.0);
        mode.add_css_class("dim-label");
        content.append(&mode);

        let source = gtk::Label::new(None);
        source.set_xalign(0.0);
        source.set_yalign(0.0);
        source.set_wrap(true);
        source.set_selectable(true);
        content.append(&source);

        let hint = gtk::Label::new(Some(
            "Translation and learning analysis will be added in M2/M3.",
        ));
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        hint.add_css_class("dim-label");
        content.append(&hint);

        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        scroller.set_child(Some(&content));
        toolbar.set_content(Some(&scroller));
        window.set_content(Some(&toolbar));

        let escape = gtk::EventControllerKey::new();
        let window_for_escape = window.downgrade();
        escape.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(window) = window_for_escape.upgrade() {
                    window.hide();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(escape);

        // GTK normally exits when its last window closes. An owned application
        // hold keeps the manual-resident process alive even while hidden.
        let keep_alive = app.hold();

        // Arrivals wake the main loop only when a trigger is actually
        // delivered; there is no polling while idle. A burst of triggers is
        // coalesced to its newest invocation, and `displayed_sequence` keeps
        // stale deliveries inert — the same fencing that will order
        // out-of-order async analysis results in M2.
        glib::spawn_future_local(async move {
            let _keep_alive = keep_alive;
            let mut displayed_sequence = 0_u64;
            while let Ok(arrival) = receiver.recv().await {
                if arrival.sequence <= displayed_sequence {
                    continue;
                }
                let mut latest = arrival;
                while let Ok(next) = receiver.try_recv() {
                    if next.sequence > latest.sequence {
                        latest = next;
                    }
                }
                displayed_sequence = latest.sequence;
                let classification = classify_selection(&latest.text);
                let label = match classification {
                    Ok(SelectionMode::Word) => "Single word",
                    Ok(SelectionMode::Passage) => "Passage",
                    Err(_) => "Empty selection",
                };
                mode.set_text(&format!("{label} · request {}", latest.sequence));
                source.set_text(&latest.text);
                // This is the same window object constructed once at startup.
                window.present();
            }
        });
        // Manual startup intentionally leaves the result window hidden until
        // the first explicit selection trigger.
    });
    app.run();
}
