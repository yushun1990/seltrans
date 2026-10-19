//! One reusable native GTK/libadwaita window, owned by the resident process.
//! Passage selections are analyzed off the GTK thread; completions are
//! fenced by invocation sequence so a stale response never overwrites a
//! newer invocation.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use adw::{glib, gtk};

use seltrans_core::analysis::{
    ExpressionNote, PassageAnalysis, TranslationProvider, VocabularyNote,
};
use seltrans_core::classifier::classify_selection;
use seltrans_core::domain::SelectionMode;
use seltrans_core::ipc::Invocation;

/// Why an analysis could not be produced, combining configuration
/// problems (resident startup) with provider failures (request time).
enum AnalysisFailure {
    NotConfigured(String),
    Provider(seltrans_core::analysis::ProviderError),
}

impl std::fmt::Display for AnalysisFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(reason) => write!(f, "translation is unavailable: {reason}"),
            Self::Provider(error) => write!(f, "{error}"),
        }
    }
}

/// A worker result tagged with the invocation it belongs to; the display
/// side drops anything whose sequence is no longer current.
struct AnalysisOutcome {
    sequence: u64,
    result: Result<PassageAnalysis, AnalysisFailure>,
}

pub fn run(
    receiver: async_channel::Receiver<Invocation>,
    provider: Result<Arc<dyn TranslationProvider>, String>,
) {
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
            .default_width(720)
            .default_height(560)
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

        let translation_heading = section_heading("Translation");
        content.append(&translation_heading);

        let loading = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .visible(false)
            .build();
        let spinner = gtk::Spinner::new();
        loading.append(&spinner);
        let loading_label = gtk::Label::new(Some("Analyzing…"));
        loading_label.add_css_class("dim-label");
        loading.append(&loading_label);
        content.append(&loading);

        let translation = gtk::Label::new(None);
        translation.set_xalign(0.0);
        translation.set_yalign(0.0);
        translation.set_wrap(true);
        translation.set_selectable(true);
        translation.set_visible(false);
        content.append(&translation);

        // Selection and translation stay fixed; only the notes area below
        // scrolls, one independent page per tab.
        let tabs = adw::ViewStack::builder().vexpand(true).build();
        tabs.set_visible(false);
        let switcher = adw::ViewSwitcher::builder()
            .policy(adw::ViewSwitcherPolicy::Wide)
            .halign(gtk::Align::Start)
            .visible(false)
            .build();
        switcher.set_stack(Some(&tabs));
        content.append(&switcher);
        content.append(&tabs);

        let (vocabulary_page, vocabulary_notes) = notes_page();
        let (expressions_page, expression_notes) = notes_page();

        let error = gtk::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.add_css_class("error");
        error.set_visible(false);
        content.append(&error);

        toolbar.set_content(Some(&content));
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

        let (outcome_sender, outcome_receiver) = async_channel::bounded::<AnalysisOutcome>(64);
        let current_sequence = Rc::new(Cell::new(0_u64));

        // Widgets are glib-refcounted; hand the outcome task its own clones.
        let outcome_spinner = spinner.clone();
        let outcome_loading = loading.clone();
        let outcome_translation = translation.clone();
        let outcome_tabs = tabs.clone();
        let outcome_switcher = switcher.clone();
        let outcome_vocabulary_page = vocabulary_page.clone();
        let outcome_vocabulary_notes = vocabulary_notes.clone();
        let outcome_expressions_page = expressions_page.clone();
        let outcome_expression_notes = expression_notes.clone();
        let outcome_error = error.clone();
        let outcome_sequence = current_sequence.clone();

        // Trigger arrivals wake the main loop only when a selection is
        // actually delivered. A burst is coalesced to its newest invocation,
        // which becomes the current sequence every later completion is
        // compared against.
        let provider = provider.clone();
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
                current_sequence.set(latest.sequence);

                let classification = classify_selection(&latest.text);
                let label = match classification {
                    Ok(SelectionMode::Word) => "Single word",
                    Ok(SelectionMode::Passage) => "Passage",
                    Err(_) => "Empty selection",
                };
                mode.set_text(&format!("{label} · request {}", latest.sequence));
                source.set_text(&latest.text);
                spinner.stop();
                translation.set_visible(false);
                switcher.set_visible(false);
                tabs.set_visible(false);
                error.set_visible(false);

                match classification {
                    Ok(SelectionMode::Passage) => {
                        // This is the same window object constructed once at
                        // startup; show the source immediately and let the
                        // analysis fill in asynchronously.
                        window.present();
                        translation_heading.set_visible(true);
                        loading.set_visible(true);
                        spinner.start();
                        loading_label.set_text("Analyzing…");
                        let sender = outcome_sender.clone();
                        let provider = provider.clone();
                        let text = latest.text.clone();
                        // The network call happens on its own worker thread;
                        // the GTK thread only ever receives the outcome.
                        std::thread::spawn(move || {
                            let result = match provider {
                                Ok(provider) => provider
                                    .analyze_passage(&text)
                                    .map_err(AnalysisFailure::Provider),
                                Err(reason) => Err(AnalysisFailure::NotConfigured(reason)),
                            };
                            let _ = sender.send_blocking(AnalysisOutcome {
                                sequence: latest.sequence,
                                result,
                            });
                        });
                    }
                    _ => {
                        // Word mode: the detailed lexical view is M4; M3
                        // intentionally leaves single-word selections alone.
                        translation_heading.set_visible(false);
                        loading.set_visible(false);
                    }
                }
            }
        });

        // Analyses complete out of band and may finish after a newer
        // trigger; only the outcome matching the current sequence is shown.
        let outcome_content = content.clone();
        glib::spawn_future_local(async move {
            while let Ok(outcome) = outcome_receiver.recv().await {
                if outcome.sequence != outcome_sequence.get() {
                    continue;
                }
                outcome_spinner.stop();
                outcome_loading.set_visible(false);
                match outcome.result {
                    Ok(analysis) => {
                        outcome_error.set_visible(false);
                        outcome_translation.set_text(&analysis.translation);
                        outcome_translation.set_visible(true);
                        render_vocabulary(&outcome_vocabulary_notes, &analysis.vocabulary);
                        render_expressions(&outcome_expression_notes, &analysis.expressions);
                        set_notes_pages(
                            &outcome_tabs,
                            &outcome_vocabulary_page,
                            &outcome_expressions_page,
                            analysis.vocabulary.len(),
                            analysis.expressions.len(),
                        );
                        outcome_switcher.set_visible(
                            !analysis.vocabulary.is_empty() && !analysis.expressions.is_empty(),
                        );
                        outcome_tabs.set_visible(
                            !analysis.vocabulary.is_empty() || !analysis.expressions.is_empty(),
                        );
                        // Repopulated wrapped labels can leave stale pixels in
                        // the GL renderer's texture cache on reused windows;
                        // force a full repaint of the result surface.
                        outcome_content.queue_draw();
                    }
                    Err(failure) => {
                        outcome_translation.set_visible(false);
                        outcome_switcher.set_visible(false);
                        outcome_tabs.set_visible(false);
                        outcome_error.set_text(&failure.to_string());
                        outcome_error.set_visible(true);
                        outcome_content.queue_draw();
                    }
                }
            }
        });
        // Manual startup intentionally leaves the result window hidden until
        // the first explicit selection trigger.
    });
    app.run();
}

fn section_heading(title: &str) -> gtk::Label {
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("title-3");
    heading.set_visible(false);
    heading
}

fn notes_list() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_bottom(12)
        .build()
}

/// One scrollable tab page: the notes box lives in its own scrolled window
/// so each tab scrolls independently of the fixed selection/translation.
fn notes_page() -> (gtk::ScrolledWindow, gtk::Box) {
    let notes = notes_list();
    let page = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    page.set_child(Some(&notes));
    (page, notes)
}

/// Rebuilds the tab pages for a new analysis. Pages are re-added rather than
/// toggled so an empty section never shows an empty tab, and the switcher is
/// only worth its space when both sections have content.
fn set_notes_pages(
    tabs: &adw::ViewStack,
    vocabulary_page: &gtk::ScrolledWindow,
    expressions_page: &gtk::ScrolledWindow,
    vocabulary_count: usize,
    expressions_count: usize,
) {
    while let Some(child) = tabs.first_child() {
        tabs.remove(&child);
    }
    if vocabulary_count > 0 {
        let page = tabs.add_titled(vocabulary_page, Some("vocabulary"), "Vocabulary");
        page.set_badge_number(vocabulary_count as u32);
    }
    if expressions_count > 0 {
        let page = tabs.add_titled(expressions_page, Some("expressions"), "Expressions");
        page.set_badge_number(expressions_count as u32);
    }
}

/// The result window is reused across invocations, so each section is
/// rebuilt from scratch before new notes are appended.
fn clear_notes(list: &gtk::Box) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn render_vocabulary(list: &gtk::Box, notes: &[VocabularyNote]) {
    clear_notes(list);
    for note in notes {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();

        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let surface = selectable_label(None);
        surface.set_markup(&format!(
            "<b>{}</b>",
            glib::markup_escape_text(&note.surface)
        ));
        title.append(&surface);
        if let Some(lemma) = note.lemma.as_deref().filter(|lemma| !lemma.is_empty())
            && lemma != note.surface
        {
            let lemma_label = selectable_label(Some(lemma));
            lemma_label.add_css_class("caption");
            lemma_label.add_css_class("dim-label");
            lemma_label.set_valign(gtk::Align::Baseline);
            title.append(&lemma_label);
        }
        row.append(&title);

        let meaning = selectable_label(Some(&note.contextual_meaning));
        row.append(&meaning);

        if let Some(reason) = note.reason.as_deref().filter(|reason| !reason.is_empty()) {
            let reason_label = selectable_label(Some(reason));
            reason_label.add_css_class("caption");
            reason_label.add_css_class("dim-label");
            row.append(&reason_label);
        }
        list.append(&row);
    }
}

fn render_expressions(list: &gtk::Box, notes: &[ExpressionNote]) {
    clear_notes(list);
    for note in notes {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();

        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let kind = gtk::Label::new(Some(&note.kind.to_string()));
        kind.add_css_class("caption");
        kind.add_css_class("dim-label");
        kind.set_valign(gtk::Align::Baseline);
        title.append(&kind);
        let surface = selectable_label(None);
        surface.set_markup(&format!(
            "<b>{}</b>",
            glib::markup_escape_text(&note.surface)
        ));
        title.append(&surface);
        row.append(&title);

        let meaning = selectable_label(Some(&note.meaning));
        row.append(&meaning);

        if let Some(explanation) = note
            .explanation
            .as_deref()
            .filter(|explanation| !explanation.is_empty())
        {
            let explanation_label = selectable_label(Some(explanation));
            explanation_label.add_css_class("caption");
            explanation_label.add_css_class("dim-label");
            row.append(&explanation_label);
        }
        list.append(&row);
    }
}

fn selectable_label(text: Option<&str>) -> gtk::Label {
    let label = gtk::Label::new(text);
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_valign(gtk::Align::Start);
    label.set_wrap(true);
    label.set_selectable(true);
    label
}
