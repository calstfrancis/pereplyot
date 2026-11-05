//! The Review window: what you chose to remember, asked back at the pace the FSRS model sets.
//! Space shows the answer, 1–4 rate it (Again, Hard, Good, Easy).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use pereplyot_core::fsrs::Rating;
use pereplyot_core::review::Card;

use crate::review_store;
use crate::ui::window::LaunchOptions;
use crate::ui::{toast, Widgets};

struct Session {
    queue: VecDeque<Card>,
    revealed: bool,
    done: usize,
}

fn big(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.set_wrap(true);
    l.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    l.set_xalign(0.0);
    l.set_selectable(true);
    l.add_css_class("title-3");
    l
}

pub fn show(widgets: &Rc<Widgets>) {
    let window = adw::Window::builder()
        .title("Review")
        .transient_for(&widgets.window)
        .default_width(620)
        .default_height(520)
        .build();
    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let enabled = gtk4::Switch::new();
    enabled.set_valign(gtk4::Align::Center);
    enabled.set_active(widgets.config.borrow().review);
    enabled.set_tooltip_text(Some("Review what is tagged #remember"));
    let on_label = gtk4::Label::new(Some("Review"));
    header.pack_start(&on_label);
    header.pack_start(&enabled);
    view.add_top_bar(&header);

    let stack = gtk4::Stack::new();
    view.set_content(Some(&stack));
    window.set_content(Some(&view));

    // Off.
    let off = adw::StatusPage::new();
    off.set_icon_name(Some("alarm-symbolic"));
    off.set_title("Review is off");
    off.set_description(Some(
        "Tag a highlight #remember (in its note) and it is asked back to you at the pace the FSRS memory \
         model sets: its sentence with your words blanked out, or a question and answer you wrote as \
         “Q: … A: …”. Your notes are never changed, and turning this off removes every card.",
    ));
    stack.add_named(&off, Some("off"));

    // Asking.
    let asking = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
    asking.set_margin_top(18);
    asking.set_margin_bottom(18);
    asking.set_margin_start(24);
    asking.set_margin_end(24);
    let count = gtk4::Label::new(None);
    count.set_xalign(0.0);
    count.add_css_class("dim-label");
    count.add_css_class("caption");
    let source = gtk4::Label::new(None);
    source.set_xalign(0.0);
    source.add_css_class("dim-label");
    let front = big("");
    let answer = big("");
    answer.add_css_class("accent");
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    let inner = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
    inner.append(&front);
    inner.append(&answer);
    scroll.set_child(Some(&inner));
    let show_answer = gtk4::Button::with_label("Show answer");
    show_answer.add_css_class("suggested-action");
    show_answer.add_css_class("pill");
    show_answer.set_halign(gtk4::Align::Center);
    let ratings = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    ratings.set_halign(gtk4::Align::Center);
    let rate_buttons: Vec<(Rating, gtk4::Button)> = Rating::ALL
        .iter()
        .map(|r| {
            let b = gtk4::Button::with_label(&format!("{}  {}", *r as u32, r.label()));
            b.add_css_class("pill");
            ratings.append(&b);
            (*r, b)
        })
        .collect();
    let context = gtk4::Button::with_label("Open in context");
    context.add_css_class("flat");
    context.set_halign(gtk4::Align::Start);
    asking.append(&count);
    asking.append(&source);
    asking.append(&scroll);
    asking.append(&show_answer);
    asking.append(&ratings);
    asking.append(&context);
    stack.add_named(&asking, Some("asking"));

    // Done.
    let done = adw::StatusPage::new();
    done.set_icon_name(Some("emblem-ok-symbolic"));
    done.set_title("Nothing due");
    stack.add_named(&done, Some("done"));

    let session = Rc::new(RefCell::new(Session {
        queue: VecDeque::new(),
        revealed: false,
        done: 0,
    }));

    let render: Rc<dyn Fn()> = {
        let (session, stack, count, source, front, answer, show_answer, ratings, done_page) = (
            session.clone(),
            stack.clone(),
            count.clone(),
            source.clone(),
            front.clone(),
            answer.clone(),
            show_answer.clone(),
            ratings.clone(),
            done.clone(),
        );
        Rc::new(move || {
            let s = session.borrow();
            let Some(card) = s.queue.front() else {
                done_page.set_description(Some(&format!(
                    "{} reviewed this sitting. Come back when more are due.",
                    s.done
                )));
                stack.set_visible_child_name("done");
                return;
            };
            stack.set_visible_child_name("asking");
            count.set_text(&format!("{} to go", s.queue.len()));
            let place = if card.label.is_empty() {
                String::new()
            } else {
                format!(", p. {}", card.label)
            };
            source.set_text(&format!("{}{place}", card.title));
            front.set_text(&card.front);
            answer.set_text(&card.back);
            answer.set_visible(s.revealed);
            show_answer.set_visible(!s.revealed);
            ratings.set_visible(s.revealed);
        })
    };

    let start: Rc<dyn Fn()> = {
        let (widgets, session, render, stack, done) = (
            widgets.clone(),
            session.clone(),
            render.clone(),
            stack.clone(),
            done.clone(),
        );
        Rc::new(move || {
            if !widgets.config.borrow().review {
                stack.set_visible_child_name("off");
                return;
            }
            let library = widgets.library.borrow().clone();
            let (session, render, stack, done) =
                (session.clone(), render.clone(), stack.clone(), done.clone());
            done.set_title("Looking for cards…");
            stack.set_visible_child_name("done");
            glib::spawn_future_local(async move {
                let due = gtk4::gio::spawn_blocking(move || {
                    review_store::sync(&library);
                    review_store::due_now()
                })
                .await
                .unwrap_or_default();
                done.set_title("Nothing due");
                if due.is_empty() {
                    let all = gtk4::gio::spawn_blocking(|| review_store::all().len())
                        .await
                        .unwrap_or(0);
                    done.set_description(Some(&if all == 0 {
                        "Add #remember to a highlight's note, and it will be asked here."
                            .to_string()
                    } else {
                        format!("{all} cards are scheduled; none are due yet.")
                    }));
                }
                let mut s = session.borrow_mut();
                s.queue = due.into();
                s.revealed = false;
                s.done = 0;
                drop(s);
                render();
            });
        })
    };

    {
        let (widgets, start, enabled_stack) = (widgets.clone(), start.clone(), stack.clone());
        enabled.connect_active_notify(move |sw| {
            let on = sw.is_active();
            {
                let mut c = widgets.config.borrow_mut();
                if c.review == on {
                    return;
                }
                c.review = on;
                c.save();
            }
            if !on {
                review_store::forget_all();
                enabled_stack.set_visible_child_name("off");
            } else {
                start();
            }
        });
    }
    {
        let (session, render) = (session.clone(), render.clone());
        show_answer.connect_clicked(move |_| {
            session.borrow_mut().revealed = true;
            render();
        });
    }
    for (rating, button) in &rate_buttons {
        let (session, render, rating) = (session.clone(), render.clone(), *rating);
        button.connect_clicked(move |_| rate(&session, &render, rating));
    }
    {
        let (widgets, session) = (widgets.clone(), session.clone());
        context.connect_clicked(move |_| {
            let card = session.borrow().queue.front().cloned();
            let Some(card) = card else { return };
            open_in_context(&widgets, &card);
        });
    }
    let keys = gtk4::EventControllerKey::new();
    {
        let (session, render, stack) = (session.clone(), render.clone(), stack.clone());
        let window = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if stack.visible_child_name().as_deref() != Some("asking") {
                if key == gtk4::gdk::Key::Escape {
                    window.close();
                }
                return glib::Propagation::Proceed;
            }
            let revealed = session.borrow().revealed;
            match key {
                gtk4::gdk::Key::space | gtk4::gdk::Key::Return if !revealed => {
                    session.borrow_mut().revealed = true;
                    render();
                    glib::Propagation::Stop
                }
                k if revealed => match k
                    .to_unicode()
                    .and_then(|c| c.to_digit(10))
                    .and_then(Rating::from_digit)
                {
                    Some(r) => {
                        rate(&session, &render, r);
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Proceed,
                },
                gtk4::gdk::Key::Escape => {
                    window.close();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    window.add_controller(keys);
    start();
    window.present();
}

fn rate(session: &Rc<RefCell<Session>>, render: &Rc<dyn Fn()>, rating: Rating) {
    let card = {
        let mut s = session.borrow_mut();
        if !s.revealed {
            return;
        }
        s.queue.pop_front()
    };
    let Some(card) = card else { return };
    let updated = review_store::rate(&card, rating);
    {
        let mut s = session.borrow_mut();
        s.done += 1;
        s.revealed = false;
        // A card forgotten comes round again before the sitting ends.
        if rating == Rating::Again {
            s.queue.push_back(updated);
        }
    }
    render();
}

fn open_in_context(widgets: &Rc<Widgets>, card: &Card) {
    let Some(entry) = fond_read_gtk::history::load()
        .into_iter()
        .find(|e| e.hash == card.hash)
    else {
        toast(widgets, "That document is no longer in your history");
        return;
    };
    crate::ui::window::open_path_with_host(
        widgets,
        entry.path,
        LaunchOptions {
            start_annotation: card.annotation.clone(),
            start_page: card.label.parse::<u32>().ok(),
            ..LaunchOptions::default()
        },
    );
}
