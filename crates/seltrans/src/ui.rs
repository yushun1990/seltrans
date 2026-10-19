//! One reusable native GTK/libadwaita window, owned by the resident process.
//! Passage selections are analyzed off the GTK thread; completions are
//! fenced by invocation sequence so a stale response never overwrites a
//! newer invocation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use adw::{glib, gtk};

use seltrans_core::analysis::{
    ExpressionKind, ExpressionNote, PassageAnalysis, TranslationProvider, VocabularyNote,
};
use seltrans_core::classifier::classify_selection;
use seltrans_core::domain::SelectionMode;
use seltrans_core::ipc::Invocation;

/// Application-level styling beyond the libadwaita defaults: section
/// eyebrows, kind pills, card padding and reading-friendly line height.
/// Only named theme colors are referenced so light, dark and high-contrast
/// styles keep working without per-variant rules here.
const APP_CSS: &str = "
.pill {
    border-radius: 9999px;
    padding-top: 2px;
    padding-bottom: 2px;
    padding-left: 10px;
    padding-right: 10px;
    background-color: alpha(@accent_color, 0.13);
    color: @accent_color;
    font-size: 85%;
    font-weight: 600;
}
.section-label {
    font-size: 82%;
    font-weight: 700;
    letter-spacing: 1.2px;
    opacity: 0.55;
}
.card-pad { padding: 16px; }
.row-pad { padding: 12px; }
.source-text { line-height: 1.5; }
";

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
        load_app_css();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Seltrans")
            .default_width(760)
            .default_height(600)
            .build();
        window.set_hide_on_close(true);

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let window_title = adw::WindowTitle::new("Seltrans", "");
        header.set_title_widget(Some(&window_title));
        let quit = gtk::Button::with_label("Quit");
        let app_for_quit = app.clone();
        quit.connect_clicked(move |_| app_for_quit.quit());
        header.pack_end(&quit);
        toolbar.add_top_bar(&header);

        // Provider failures and empty selections surface as a banner with
        // one-click retry, spanning the window above the content.
        let banner = adw::Banner::builder().revealed(false).build();
        toolbar.add_top_bar(&banner);

        // Before the first trigger the window is a guided placeholder, not
        // an empty frame; every trigger swaps to the result view.
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title("No selection yet")
            .description("Select English text anywhere and press Alt+Q. The analysis appears here.")
            .build();
        stack.add_named(&empty, Some("empty"));

        // The reading column keeps a comfortable text measure on wide
        // windows instead of stretching to the full window width.
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(22)
            .margin_end(22)
            .build();
        let clamp = adw::Clamp::builder()
            .maximum_size(720)
            .tightening_threshold(580)
            .build();
        clamp.set_child(Some(&content));
        stack.add_named(&clamp, Some("result"));
        stack.set_visible_child_name("empty");

        let source_card = card();
        let source_section = section_label("SELECTION");
        source_card.append(&source_section);
        let source = selectable_label(None);
        source.add_css_class("source-text");
        source_card.append(&source);
        content.append(&source_card);

        let translation_card = card();
        translation_card.set_visible(false);
        translation_card.append(&section_label("TRANSLATION"));
        let translation = selectable_label(None);
        translation_card.append(&translation);
        content.append(&translation_card);

        let loading = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .visible(false)
            .build();
        let spinner = gtk::Spinner::new();
        loading.append(&spinner);
        let loading_label = gtk::Label::new(Some("Analyzing…"));
        loading_label.add_css_class("dim-label");
        loading.append(&loading_label);
        content.append(&loading);

        // Word mode: the detailed lexical view is M4; M3 intentionally
        // leaves single-word selections alone beyond showing the word.
        let word_hint = gtk::Label::new(Some(
            "The detailed single-word view arrives in the next milestone.",
        ));
        word_hint.set_xalign(0.0);
        word_hint.set_wrap(true);
        word_hint.add_css_class("dim-label");
        word_hint.set_visible(false);
        content.append(&word_hint);

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

        toolbar.set_content(Some(&stack));
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
        // The passage belonging to the current sequence, kept so the error
        // banner's Retry action can replay it without a new selection.
        let last_passage = Rc::new(RefCell::new(None::<(u64, String)>));

        // Retry re-runs the last passage under the sequence that is current
        // at click time; a newer trigger still fences the older retry away.
        let retry_banner = banner.clone();
        let retry_loading = loading.clone();
        let retry_spinner = spinner.clone();
        let retry_translation_card = translation_card.clone();
        let retry_last_passage = last_passage.clone();
        let retry_provider = provider.clone();
        let retry_sender = outcome_sender.clone();
        banner.connect_button_clicked(move |_| {
            let Some((sequence, text)) = retry_last_passage.borrow().clone() else {
                return;
            };
            retry_banner.set_revealed(false);
            retry_translation_card.set_visible(false);
            retry_loading.set_visible(true);
            retry_spinner.start();
            spawn_analysis(&retry_provider, &retry_sender, sequence, text);
        });

        // Widgets are glib-refcounted; hand each task its own clones.
        let arrival_window = window.clone();
        let arrival_stack = stack.clone();
        let arrival_title = window_title.clone();
        let arrival_banner = banner.clone();
        let arrival_source_section = source_section.clone();
        let arrival_source = source.clone();
        let arrival_translation_card = translation_card.clone();
        let arrival_loading = loading.clone();
        let arrival_spinner = spinner.clone();
        let arrival_word_hint = word_hint.clone();
        let arrival_switcher = switcher.clone();
        let arrival_tabs = tabs.clone();
        let arrival_last_passage = last_passage.clone();
        let arrival_sequence = current_sequence.clone();

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
                arrival_sequence.set(latest.sequence);

                match classify_selection(&latest.text) {
                    Err(_) => {
                        // A stray shortcut must not blank the current
                        // result; flag it and leave the view untouched.
                        arrival_banner.set_button_label(None);
                        arrival_banner.set_title("The selection is empty");
                        arrival_banner.set_revealed(true);
                        continue;
                    }
                    Ok(SelectionMode::Word) => {
                        // This is the same window object constructed once at
                        // startup; the word shows immediately, M4 fills in
                        // the detailed lexical view.
                        arrival_window.present();
                        arrival_stack.set_visible_child_name("result");
                        arrival_title.set_subtitle("Single word");
                        arrival_source_section.set_text("SELECTION");
                        arrival_source.set_text(&latest.text);
                        arrival_banner.set_revealed(false);
                        arrival_spinner.stop();
                        arrival_loading.set_visible(false);
                        arrival_translation_card.set_visible(false);
                        arrival_switcher.set_visible(false);
                        arrival_tabs.set_visible(false);
                        arrival_word_hint.set_visible(true);
                    }
                    Ok(SelectionMode::Passage) => {
                        // Show the source immediately and let the analysis
                        // fill in asynchronously.
                        arrival_window.present();
                        arrival_stack.set_visible_child_name("result");
                        arrival_title.set_subtitle("Passage");
                        arrival_source_section.set_text("PASSAGE");
                        arrival_source.set_text(&latest.text);
                        arrival_banner.set_revealed(false);
                        arrival_spinner.stop();
                        arrival_word_hint.set_visible(false);
                        arrival_translation_card.set_visible(false);
                        arrival_switcher.set_visible(false);
                        arrival_tabs.set_visible(false);
                        arrival_loading.set_visible(true);
                        arrival_spinner.start();
                        arrival_last_passage
                            .borrow_mut()
                            .replace((latest.sequence, latest.text.clone()));
                        spawn_analysis(&provider, &outcome_sender, latest.sequence, latest.text);
                    }
                }
            }
        });

        // Analyses complete out of band and may finish after a newer
        // trigger; only the outcome matching the current sequence is shown.
        let outcome_content = content.clone();
        let outcome_banner = banner.clone();
        let outcome_spinner = spinner.clone();
        let outcome_loading = loading.clone();
        let outcome_translation_card = translation_card.clone();
        let outcome_translation = translation.clone();
        let outcome_tabs = tabs.clone();
        let outcome_switcher = switcher.clone();
        let outcome_vocabulary_page = vocabulary_page.clone();
        let outcome_vocabulary_notes = vocabulary_notes.clone();
        let outcome_expressions_page = expressions_page.clone();
        let outcome_expression_notes = expression_notes.clone();
        let outcome_sequence = current_sequence.clone();
        glib::spawn_future_local(async move {
            while let Ok(outcome) = outcome_receiver.recv().await {
                if outcome.sequence != outcome_sequence.get() {
                    continue;
                }
                outcome_spinner.stop();
                outcome_loading.set_visible(false);
                match outcome.result {
                    Ok(analysis) => {
                        outcome_banner.set_revealed(false);
                        outcome_translation.set_text(&analysis.translation);
                        outcome_translation_card.set_visible(true);
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
                        outcome_translation_card.set_visible(false);
                        outcome_switcher.set_visible(false);
                        outcome_tabs.set_visible(false);
                        outcome_banner.set_title(&failure.to_string());
                        outcome_banner.set_button_label(Some("Retry"));
                        outcome_banner.set_revealed(true);
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

/// The network call happens on its own worker thread; the GTK thread only
/// ever receives the outcome tagged with the sequence it belongs to.
fn spawn_analysis(
    provider: &Result<Arc<dyn TranslationProvider>, String>,
    sender: &async_channel::Sender<AnalysisOutcome>,
    sequence: u64,
    text: String,
) {
    let provider = provider.clone();
    let sender = sender.clone();
    std::thread::spawn(move || {
        let result = match provider {
            Ok(provider) => provider
                .analyze_passage(&text)
                .map_err(AnalysisFailure::Provider),
            Err(reason) => Err(AnalysisFailure::NotConfigured(reason)),
        };
        let _ = sender.send_blocking(AnalysisOutcome { sequence, result });
    });
}

fn load_app_css() {
    let provider = gtk::CssProvider::new();
    // load_from_string needs the v4_12 gtk feature; without it the
    // equivalent data loader is not yet deprecated.
    provider.load_from_data(APP_CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn card() -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    card.add_css_class("card");
    card.add_css_class("card-pad");
    card
}

fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("section-label");
    label
}

fn notes_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    // With only a few notes the boxed background should not stretch down
    // the whole tab page.
    list.set_valign(gtk::Align::Start);
    list.set_hexpand(true);
    list
}

/// One scrollable tab page: the notes list lives in its own scrolled window
/// so each tab scrolls independently of the fixed selection/translation.
fn notes_page() -> (gtk::ScrolledWindow, gtk::ListBox) {
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
fn clear_notes(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn note_row(content: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    // Reading notes are content, not a choosable list: no hover/selection
    // styling on rows.
    row.set_activatable(false);
    row.set_selectable(false);
    row.set_child(Some(content));
    row
}

fn note_row_box() -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    row.add_css_class("row-pad");
    row
}

/// A rounded accent-colored tag naming what kind of expression this is.
fn kind_pill(kind: &ExpressionKind) -> gtk::Label {
    let pill = gtk::Label::new(Some(&kind.to_string()));
    pill.add_css_class("pill");
    pill.set_valign(gtk::Align::Baseline);
    pill
}

fn render_vocabulary(list: &gtk::ListBox, notes: &[VocabularyNote]) {
    clear_notes(list);
    for note in notes {
        let row_content = note_row_box();

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
        row_content.append(&title);

        let meaning = selectable_label(Some(&note.contextual_meaning));
        row_content.append(&meaning);

        if let Some(reason) = note.reason.as_deref().filter(|reason| !reason.is_empty()) {
            let reason_label = selectable_label(Some(reason));
            reason_label.add_css_class("caption");
            reason_label.add_css_class("dim-label");
            row_content.append(&reason_label);
        }
        list.append(&note_row(&row_content));
    }
}

fn render_expressions(list: &gtk::ListBox, notes: &[ExpressionNote]) {
    clear_notes(list);
    for note in notes {
        let row_content = note_row_box();

        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        title.append(&kind_pill(&note.kind));
        let surface = selectable_label(None);
        surface.set_markup(&format!(
            "<b>{}</b>",
            glib::markup_escape_text(&note.surface)
        ));
        title.append(&surface);
        row_content.append(&title);

        let meaning = selectable_label(Some(&note.meaning));
        row_content.append(&meaning);

        if let Some(explanation) = note
            .explanation
            .as_deref()
            .filter(|explanation| !explanation.is_empty())
        {
            let explanation_label = selectable_label(Some(explanation));
            explanation_label.add_css_class("caption");
            explanation_label.add_css_class("dim-label");
            row_content.append(&explanation_label);
        }
        list.append(&note_row(&row_content));
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
