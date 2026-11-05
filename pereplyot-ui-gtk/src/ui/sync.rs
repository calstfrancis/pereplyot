//! The Sync folder window: keep Pereplyot's data in a folder Syncthing, Nextcloud or Dropbox shares, and
//! merge the conflict copies they leave behind.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::sync_folder::{self, State};
use crate::ui::{toast, Widgets};

fn describe(state: &State) -> String {
    match state {
        State::Local => "Your notes are kept on this computer only.".to_string(),
        State::Linked(path) => format!(
            "Your notes are kept in {}, which your sync client shares.",
            path.display()
        ),
    }
}

pub fn show(widgets: &Rc<Widgets>) {
    let window = adw::Window::builder()
        .title("Sync folder")
        .transient_for(&widgets.window)
        .modal(true)
        .default_width(520)
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();

    let about = adw::PreferencesGroup::new();
    about.set_description(Some(
        "Pereplyot has no server and no account. To read on two computers, put its data in a folder a sync \
         client you already use (Syncthing, Nextcloud, Dropbox) shares between them. If both change the \
         same notes at once, Pereplyot merges the copies the client leaves: every mark is kept, and the \
         later edit of any one wins. Nothing is deleted; merged copies are set aside in a “conflicts-merged” \
         folder.",
    ));
    let status = adw::ActionRow::new();
    status.set_title("Where your data is");
    status.set_subtitle(&describe(&sync_folder::state()));
    status.set_subtitle_lines(3);
    about.add(&status);

    let actions = adw::PreferencesGroup::new();
    let choose = adw::ActionRow::new();
    choose.set_title("Use a sync folder…");
    choose.set_subtitle("Copies your data there and links to it. Restart Pereplyot afterwards.");
    choose.set_activatable(true);
    let stop = adw::ActionRow::new();
    stop.set_title("Stop syncing");
    stop.set_subtitle("Takes a copy back to this computer. The sync folder is left as it is.");
    stop.set_activatable(true);
    let merge = adw::ActionRow::new();
    merge.set_title("Merge conflicts now");
    merge.set_activatable(true);
    let linked = matches!(sync_folder::state(), State::Linked(_));
    choose.set_visible(!linked);
    stop.set_visible(linked);
    actions.add(&choose);
    actions.add(&stop);
    actions.add(&merge);
    page.add(&about);
    page.add(&actions);
    view.set_content(Some(&page));
    window.set_content(Some(&view));

    {
        let (widgets, window, status) = (widgets.clone(), window.clone(), status.clone());
        choose.connect_activated(move |_| {
            let dialog = gtk4::FileDialog::builder()
                .title("Choose the sync folder")
                .build();
            let (widgets, window, status) = (widgets.clone(), window.clone(), status.clone());
            dialog.select_folder(
                Some(&window.clone()),
                gtk4::gio::Cancellable::NONE,
                move |result| {
                    let Some(folder) = result.ok().and_then(|f| f.path()) else {
                        return;
                    };
                    let (widgets, status) = (widgets.clone(), status.clone());
                    glib::spawn_future_local(async move {
                        let done =
                            gtk4::gio::spawn_blocking(move || sync_folder::use_folder(&folder))
                                .await;
                        match done {
                            Ok(Ok(())) => {
                                status.set_subtitle(&describe(&sync_folder::state()));
                                toast(&widgets, "Done. Restart Pereplyot to use the sync folder");
                            }
                            Ok(Err(e)) => {
                                toast(&widgets, &format!("Couldn't use that folder: {e}"))
                            }
                            Err(_) => toast(&widgets, "Couldn't use that folder"),
                        }
                    });
                },
            );
        });
    }
    {
        let (widgets, status) = (widgets.clone(), status.clone());
        stop.connect_activated(move |_| match sync_folder::stop() {
            Ok(()) => {
                status.set_subtitle(&describe(&sync_folder::state()));
                toast(&widgets, "Done. Restart Pereplyot to use the local copy");
            }
            Err(e) => toast(&widgets, &format!("Couldn't stop syncing: {e}")),
        });
    }
    {
        let widgets = widgets.clone();
        merge.connect_activated(move |_| {
            let widgets = widgets.clone();
            glib::spawn_future_local(async move {
                let report = gtk4::gio::spawn_blocking(|| {
                    sync_folder::resolve_conflicts(&crate::paths::data_dir())
                })
                .await
                .unwrap_or_default();
                toast(
                    &widgets,
                    &if report.merged + report.settled == 0 {
                        "No conflicting copies found".to_string()
                    } else {
                        format!(
                            "Merged {} set(s) of notes and settled {} other file(s)",
                            report.merged, report.settled
                        )
                    },
                );
            });
        });
    }
    window.present();
}
