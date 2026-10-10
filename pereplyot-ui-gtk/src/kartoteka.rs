use std::path::Path;

use gtk4::{gio, glib, prelude::*};

const APP_ID: &str = "io.github.calstfrancis.Kartoteka";

pub fn hand_off(path: &Path) {
    let file = path.to_path_buf();
    glib::spawn_future_local(async move {
        let note = match open_over_dbus(&file).await {
            Ok(()) => "Sent to Kartoteka — it adds the file to its open library".to_string(),
            Err(_) => match std::process::Command::new("kartoteka-gtk")
                .arg(&file)
                .spawn()
            {
                Ok(_) => "Sent to Kartoteka".to_string(),
                Err(_) => "Kartoteka is not installed here".to_string(),
            },
        };
        fond_read_gtk::reader_host::toast_in_readers(&note);
    });
}

async fn open_over_dbus(file: &Path) -> Result<(), glib::Error> {
    let connection = gio::bus_get_future(gio::BusType::Session).await?;
    let uri = gio::File::for_path(file).uri().to_string();
    let args = (
        vec![uri],
        std::collections::HashMap::<String, glib::Variant>::new(),
    );
    connection
        .call_future(
            Some(APP_ID),
            "/io/github/calstfrancis/Kartoteka",
            "org.freedesktop.Application",
            "Open",
            Some(&args.to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            30_000,
        )
        .await?;
    Ok(())
}
